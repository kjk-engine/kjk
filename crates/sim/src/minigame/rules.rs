//! A minigame as its level has it (the ARE's `MiniGame` struct, the tracks'
//! paths from their models, the LYT's places): fixed for the module, in the
//! level's digest. What changes in play is the `Course`.

use crate::math::Vec3;
use crate::wire::Writer;

/// Which kind of minigame: the ARE's `Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 1: the player moves along its track and across it (a swoop race).
    Swoop,
    /// 2: the player turns where its track holds it (the Ebon Hawk's gun).
    Turret,
}

/// The script slots of a follower's (and an obstacle's) `Scripts` struct:
/// K1's ten, and the three TSL's adds (its executable runs the player's
/// `OnAccelerate` and `OnBrake` on the swoop's keys; nothing raises
/// `OnHitWorld`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Create,
    Heartbeat,
    Damage,
    Death,
    Fire,
    HitBullet,
    HitFollower,
    HitObstacle,
    TrackLoop,
    AnimEvent,
    Accelerate,
    Brake,
    HitWorld,
}

impl Hook {
    pub const ALL: [Hook; 13] = [
        Hook::Create,
        Hook::Heartbeat,
        Hook::Damage,
        Hook::Death,
        Hook::Fire,
        Hook::HitBullet,
        Hook::HitFollower,
        Hook::HitObstacle,
        Hook::TrackLoop,
        Hook::AnimEvent,
        Hook::Accelerate,
        Hook::Brake,
        Hook::HitWorld,
    ];

    /// The ARE's label for it.
    pub fn label(self) -> &'static str {
        match self {
            Hook::Create => "OnCreate",
            Hook::Heartbeat => "OnHeartbeat",
            Hook::Damage => "OnDamage",
            Hook::Death => "OnDeath",
            Hook::Fire => "OnFire",
            Hook::HitBullet => "OnHitBullet",
            Hook::HitFollower => "OnHitFollower",
            Hook::HitObstacle => "OnHitObstacle",
            Hook::TrackLoop => "OnTrackLoop",
            Hook::AnimEvent => "OnAnimEvent",
            Hook::Accelerate => "OnAccelerate",
            Hook::Brake => "OnBrake",
            Hook::HitWorld => "OnHitWorld",
        }
    }
}

/// A follower's or obstacle's scripts by `Hook`, empty for none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hooks(pub [String; 13]);

impl Hooks {
    /// From (label, script) pairs as the ARE names them.
    pub fn from_labels(scripts: &[(String, String)]) -> Hooks {
        let mut hooks = Hooks::default();
        for (label, script) in scripts {
            if let Some(hook) = Hook::ALL.iter().find(|hook| hook.label() == label) {
                hooks.0[*hook as usize] = script.clone();
            }
        }
        hooks
    }

    pub fn get(&self, hook: Hook) -> Option<&str> {
        let script = &self.0[hook as usize];
        (!script.is_empty()).then_some(script.as_str())
    }
}

/// A track: where its `modelhook` node goes over the track model's `track`
/// clip, from where the LYT places the model (metres; seconds).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    /// The LYT's place plus the hook's rest position.
    pub origin: Vec3,
    /// The hook's rest turn (x, y, z, w).
    pub rest: [f32; 4],
    /// Offsets from the rest position, by time.
    pub positions: Vec<(f32, Vec3)>,
    /// Each position key's bezier handles, in and out, from its value (the
    /// turret fighters' curving tracks); empty for a track keyed in lines.
    pub handles: Vec<(Vec3, Vec3)>,
    /// Turns (x, y, z, w), by time.
    pub orientations: Vec<(f32, [f32; 4])>,
    /// The clip's length; 0 for a still track.
    pub length: f32,
}

impl Path {
    /// Where the track is `t` seconds in (held at its ends): along each
    /// key's bezier curve when it has handles (from a key, by its out
    /// handle, to the next by that one's in handle, each relative to its
    /// key) [verify: the handles' order and sign], else straight.
    pub fn at(&self, t: f32) -> Vec3 {
        let offset = if self.handles.len() == self.positions.len() && !self.handles.is_empty() {
            self.curve(t)
        } else {
            sample(&self.positions, t, |a, b, f| {
                [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f)
            })
        }
        .unwrap_or([0.0; 3]);
        [0, 1, 2].map(|i| self.origin[i] + offset[i])
    }

    fn curve(&self, t: f32) -> Option<Vec3> {
        let (first, last) = (self.positions.first()?, self.positions.last()?);
        if t <= first.0 {
            return Some(first.1);
        }
        if t >= last.0 {
            return Some(last.1);
        }
        let next = self.positions.iter().position(|(time, _)| *time > t)?;
        let ((ta, a), (tb, b)) = (self.positions[next - 1], self.positions[next]);
        let f = (t - ta) / (tb - ta).max(f32::EPSILON);
        let (out, into) = (self.handles[next - 1].1, self.handles[next].0);
        let g = 1.0 - f;
        Some([0, 1, 2].map(|i| {
            let (p1, p2) = (a[i] + out[i], b[i] + into[i]);
            g * g * g * a[i] + 3.0 * g * g * f * p1 + 3.0 * g * f * f * p2 + f * f * f * b[i]
        }))
    }

    /// How the track turns `t` seconds in (x, y, z, w): for drawing.
    pub fn turn(&self, t: f32) -> [f32; 4] {
        sample(&self.orientations, t, |a, b, f| {
            let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
            let sign = if dot < 0.0 { -1.0 } else { 1.0 };
            let q = [0, 1, 2, 3].map(|i| a[i] * (1.0 - f) + sign * b[i] * f);
            let length = q
                .iter()
                .map(|v| v * v)
                .sum::<f32>()
                .sqrt()
                .max(f32::EPSILON);
            q.map(|v| v / length)
        })
        .unwrap_or(self.rest)
    }

    fn digest_into(&self, out: &mut Writer) {
        vec3(out, self.origin);
        for value in self.rest {
            out.f32(value);
        }
        out.u32(self.positions.len() as u32);
        for (time, offset) in &self.positions {
            out.f32(*time);
            vec3(out, *offset);
        }
        out.u32(self.handles.len() as u32);
        for (into, out_handle) in &self.handles {
            vec3(out, *into);
            vec3(out, *out_handle);
        }
        out.u32(self.orientations.len() as u32);
        for (time, turn) in &self.orientations {
            out.f32(*time);
            for value in turn {
                out.f32(*value);
            }
        }
        out.f32(self.length);
    }
}

/// Linear between keys, held at the ends.
fn sample<T: Copy>(keys: &[(f32, T)], t: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let (first, last) = (keys.first()?, keys.last()?);
    if t <= first.0 {
        return Some(first.1);
    }
    if t >= last.0 {
        return Some(last.1);
    }
    let next = keys.iter().position(|(time, _)| *time > t)?;
    let (a, b) = (keys[next - 1], keys[next]);
    Some(mix(a.1, b.1, (t - a.0) / (b.0 - a.0).max(f32::EPSILON)))
}

/// A gun bank: what it shoots and how often.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bank {
    pub id: u32,
    pub gun_model: String,
    pub bullet_model: String,
    pub damage: i32,
    pub lifespan_ms: u32,
    pub rate_ms: u32,
    /// Metres a second.
    pub speed: f32,
    /// Whom its bullets hit: bit 0 the player, bit 1 the enemies [verify].
    pub target: u8,
    pub fire_sound: String,
    pub hit_sound: String,
    /// How far a shot strays, as a fraction of its distance.
    pub inaccuracy: f32,
    /// How near the player must be for an enemy's bank to fire (metres).
    pub sensing: f32,
    /// The cone it fires in, across and up (degrees).
    pub spread: [f32; 2],
    /// Where its bullets leave, in its follower's space: its gun model's
    /// `bullethook0` hung on the follower's `gunbank<id>` node (the
    /// turret's two barrels); the follower's middle when it has none.
    pub muzzle: Vec3,
    /// Which way its bullets leave, in its follower's space: its
    /// `gunbank<id>` node's +y (the turret's barrels, tilted 5° down and
    /// toed in so their bolts meet the crosshair's line); +y without it.
    pub way: Vec3,
}

impl Bank {
    /// Whether it shoots anything: the swoop's banks are placeholders.
    pub fn live(&self) -> bool {
        self.damage > 0 && self.speed > 0.0 && self.rate_ms > 0
    }
}

/// Bullets' target bits.
pub mod targets {
    pub const PLAYER: u8 = 1;
    pub const ENEMIES: u8 = 2;
}

/// Anything that follows a track: the player, an enemy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Follower {
    /// Its name, as `SWMG_GetObjectName` gives it: its track model's, as
    /// the MDL spells it (`211TEL_MGM05`: the races' scripts tell a mine's
    /// track, "MGM", from a pad's by it).
    pub name: String,
    pub path: Path,
    /// Times through its track; -1 for ever.
    pub loops: i32,
    pub radius: f32,
    pub invince_ms: u32,
    pub hp: i32,
    pub max_hp: i32,
    /// What it deals one it bumps into (`Bump_Damage`, `DoBumping`).
    pub bump_damage: i32,
    /// (model, turns with the follower's offset).
    pub models: Vec<(String, bool)>,
    /// Each model's clips (lowercase): a clip played on the follower plays
    /// on the models that have it, in place of what each played.
    pub clips: Vec<Vec<String>>,
    pub hooks: Hooks,
    /// The player's camera rig (the turret's); empty for none.
    pub camera: String,
    pub camera_rotate: bool,
    pub min_speed: f32,
    pub max_speed: f32,
    pub start_offset: Vec3,
    pub tunnel_infinite: [bool; 3],
    pub tunnel_pos: Vec3,
    pub tunnel_neg: Vec3,
    pub target_offset: Vec3,
    /// A follower the player passes through (a swoop's pad).
    pub trigger: bool,
    pub banks: Vec<Bank>,
    pub death_sound: String,
    pub engine_sound: String,
}

/// A still thing on the course (a swoop's buoy).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Obstacle {
    pub model: String,
    /// Its name, as `SWMG_GetObjectName` gives it: its model's, as the MDL
    /// spells it.
    pub name: String,
    /// Where its model stands (the LYT's place).
    pub position: Vec3,
    /// Its `hitbump` box: the middle, and half its size along x, y and z
    /// (metres). K1's races meet it across the ground; TSL's executable
    /// meets the mesh where the player's middle passes into it.
    pub centre: Vec3,
    pub extent: Vec3,
    pub hooks: Hooks,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    pub kind: Kind,
    /// The speed at which the player's track plays at its own pace.
    pub movement_per_sec: f32,
    /// How fast the player moves across its track (metres a second, a
    /// second), until a script sets it.
    pub lateral_accel: f32,
    pub use_inertia: bool,
    pub do_bumping: bool,
    /// Near and far clip (metres), the camera's view angle (degrees).
    pub clip: [f32; 2],
    pub view_angle: f32,
    pub music: String,
    /// Which offset axis (0 x, 1 y, 2 z) the view's yaw and pitch turn,
    /// and whether each is flipped.
    pub mouse: [u8; 2],
    pub flip: [bool; 2],
    pub player: Follower,
    pub enemies: Vec<Follower>,
    pub obstacles: Vec<Obstacle>,
    /// What each gunner past the first adds to the enemies' hit points, as
    /// a share of their maximum: the module's encounter file's `per_player
    /// hp` (M11); 0 without one.
    pub per_player_hp: f32,
    /// The ARE is TSL's (its `Scripts` structs carry `OnAccelerate`,
    /// `OnBrake` and `OnHitWorld`, set or not): the rules TSL's executable
    /// adds to K1's hold (the swoop's jump falls, the lateral acceleration
    /// a script sets is scaled by the player's Dexterity, only a negative
    /// maximum speed is refused, a race starts `FORFEIT_SWOOPRACE` false).
    pub tsl: bool,
}

impl Rules {
    /// A minigame of `kind` with only its player, at the defaults: one
    /// made of no game's data (probes and tests).
    pub fn bare(kind: Kind) -> Rules {
        Rules {
            kind,
            movement_per_sec: 0.0,
            lateral_accel: 0.0,
            use_inertia: false,
            do_bumping: false,
            clip: [0.1, 1000.0],
            view_angle: 60.0,
            music: String::new(),
            mouse: [2, 0],
            flip: [false; 2],
            player: Follower::default(),
            enemies: Vec::new(),
            obstacles: Vec::new(),
            per_player_hp: 0.0,
            tsl: false,
        }
    }

    /// Follower `n`: 0 the player, then the enemies.
    pub fn follower(&self, n: usize) -> Option<&Follower> {
        match n {
            0 => Some(&self.player),
            n => self.enemies.get(n - 1),
        }
    }

    /// How many followers there are (the player and the enemies).
    pub fn followers(&self) -> usize {
        1 + self.enemies.len()
    }

    /// Whether every model of thing `n` that has clip `under` has clip
    /// `over` too: played in place of the others, `over` then hides
    /// `under` on all of them. Without the models' clips known, it does.
    pub fn covers(&self, n: usize, over: &str, under: &str) -> bool {
        let Some(follower) = self.follower(n) else {
            return true;
        };
        let has = |clips: &Vec<String>, name: &str| {
            clips.iter().any(|clip| clip.eq_ignore_ascii_case(name))
        };
        follower
            .clips
            .iter()
            .all(|clips| !has(clips, under) || has(clips, over))
    }

    /// Thing `n`'s hooks: a follower's, then the obstacles'.
    pub fn hooks(&self, n: usize) -> Option<&Hooks> {
        match self.follower(n) {
            Some(follower) => Some(&follower.hooks),
            None => self
                .obstacles
                .get(n - self.followers())
                .map(|obstacle| &obstacle.hooks),
        }
    }

    /// Every script the minigame hooks, for the module's library.
    pub fn scripts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(&self.player)
            .chain(&self.enemies)
            .map(|follower| &follower.hooks)
            .chain(self.obstacles.iter().map(|obstacle| &obstacle.hooks))
            .flat_map(|hooks| hooks.0.iter())
            .filter(|script| !script.is_empty())
            .map(String::as_str)
    }

    /// BLAKE3 of every fact above: two machines with the same digest play
    /// the same minigame.
    pub fn digest(&self) -> [u8; 32] {
        let mut out = Writer::default();
        out.bytes(b"KJKG");
        out.u32(2);
        out.u8(self.kind as u8);
        out.f32(self.movement_per_sec);
        out.f32(self.lateral_accel);
        out.u8(u8::from(self.use_inertia) | u8::from(self.do_bumping) << 1);
        out.f32(self.clip[0]);
        out.f32(self.clip[1]);
        out.f32(self.view_angle);
        out.string(&self.music);
        out.bytes(&self.mouse);
        out.u8(u8::from(self.flip[0]) | u8::from(self.flip[1]) << 1);
        out.u32(self.followers() as u32);
        for follower in std::iter::once(&self.player).chain(&self.enemies) {
            follower.digest_into(&mut out);
        }
        out.u32(self.obstacles.len() as u32);
        for obstacle in &self.obstacles {
            out.string(&obstacle.model);
            out.string(&obstacle.name);
            vec3(&mut out, obstacle.position);
            vec3(&mut out, obstacle.centre);
            vec3(&mut out, obstacle.extent);
            hooks(&mut out, &obstacle.hooks);
        }
        out.f32(self.per_player_hp);
        out.u8(u8::from(self.tsl));
        *blake3::hash(&out.0).as_bytes()
    }
}

impl Follower {
    /// Its live banks' barrels in its own space: their muzzles' middle, and
    /// the way they meet (their ways' mean: the turret's crosshair line;
    /// +y when none).
    pub fn barrels(&self) -> (Vec3, Vec3) {
        let live = || self.banks.iter().filter(|bank| bank.live());
        let sum = |pick: fn(&Bank) -> Vec3| {
            live().fold([0.0; 3], |sum, bank| crate::math::add(sum, pick(bank)))
        };
        let count = live().count().max(1) as f32;
        let middle = sum(|bank| bank.muzzle).map(|v| v / count);
        let mut way = sum(|bank| bank.way);
        if crate::math::normalize(&mut way) <= f32::EPSILON {
            way = [0.0, 1.0, 0.0];
        }
        (middle, way)
    }

    fn digest_into(&self, out: &mut Writer) {
        out.string(&self.name);
        self.path.digest_into(out);
        out.u32(self.loops as u32);
        out.f32(self.radius);
        out.u32(self.invince_ms);
        out.u32(self.hp as u32);
        out.u32(self.max_hp as u32);
        out.u32(self.bump_damage as u32);
        out.u32(self.models.len() as u32);
        for (model, rotating) in &self.models {
            out.string(model);
            out.u8(u8::from(*rotating));
        }
        for clips in &self.clips {
            out.u32(clips.len() as u32);
            for clip in clips {
                out.string(clip);
            }
        }
        hooks(out, &self.hooks);
        out.string(&self.camera);
        out.u8(u8::from(self.camera_rotate));
        out.f32(self.min_speed);
        out.f32(self.max_speed);
        vec3(out, self.start_offset);
        for infinite in self.tunnel_infinite {
            out.u8(u8::from(infinite));
        }
        vec3(out, self.tunnel_pos);
        vec3(out, self.tunnel_neg);
        vec3(out, self.target_offset);
        out.u8(u8::from(self.trigger));
        out.u32(self.banks.len() as u32);
        for bank in &self.banks {
            out.u32(bank.id);
            out.string(&bank.gun_model);
            out.string(&bank.bullet_model);
            out.u32(bank.damage as u32);
            out.u32(bank.lifespan_ms);
            out.u32(bank.rate_ms);
            out.f32(bank.speed);
            out.u8(bank.target);
            out.string(&bank.fire_sound);
            out.string(&bank.hit_sound);
            out.f32(bank.inaccuracy);
            out.f32(bank.sensing);
            out.f32(bank.spread[0]);
            out.f32(bank.spread[1]);
            vec3(out, bank.muzzle);
            vec3(out, bank.way);
        }
        out.string(&self.death_sound);
        out.string(&self.engine_sound);
    }
}

fn vec3(out: &mut Writer, v: Vec3) {
    for value in v {
        out.f32(value);
    }
}

fn hooks(out: &mut Writer, hooks: &Hooks) {
    for script in &hooks.0 {
        out.string(script);
    }
}
