//! `m1` and `m3`: a body moved by `move_body`, step by step, in a test world
//! (`geometry`), alone or with a fight (`motion::Arms`: a Force pool for
//! force jumps, fists, sabers, blasters). `m1`'s random scenarios walk,
//! run, jump and fall about every world (now and then with a fight);
//! `m3`'s fight.
//!
//! Directives, in order (`docs/EQUIVALENCE.md`):
//! - the world: `world NAME`, and shapes on it (`box`, `ramp`, `prism`,
//!   `body`, `spawn`); `anims stand_in|varied|sparse`;
//! - the body: `start X Y Z YAW`, `time MS`, `set FIELD VALUES`;
//! - a fight: `arms KIND [key=value]` (`none fists saber dual staff
//!   saber_fists pistol rifle pistols rifle_saber saber_pistol`), `rng SEED
//!   STREAM`, `other ENTITY ANIM`, `enemy X Y Z|none`, `saber key=value`,
//!   `force key=value`, `knockdown MS`, `standup`;
//! - steps: `step [N] [key=value]`, `carry X Y Z VX VY VZ YAW LEGS
//!   [key=value]`, `react ANIM torso|both`, `start_torso ANIM SENT`,
//!   `hold ANIM torso|both [MSEC]`, `query stopping_distance S|view_of P Y
//!   R DP DY DR|shrugs_off_pain|anims_digest`.

use collision::{Bodies, BrushWorld};

use super::geometry::{self, Geometry};
use super::record::{Record, Trace};
use super::scenario::{Line, Scenario, float, int, pairs, vec3};
use crate::combat::saber::throw::Flight;
use crate::combat::saber::{
    Blade, Block, Blocked, Change, Getup, Hand, Held, Kit, Move as SaberMove, Style, Throw, Thrown,
};
use crate::combat::sheet::Blaster;
use crate::combat::{Fighter, Sheath};
use crate::math::{self, Vec3};
use crate::motion::{self, Anim, AnimInfo, AnimTable, Arms, Ground, MotionState, Other};
use crate::snapshot::SimId;
use crate::wire::Writer;
use crate::{Command, SimRng, buttons};

/// A fight's state between steps.
struct Fight {
    fighter: Fighter,
    kit: Kit,
    rng: SimRng,
    others: Vec<Other>,
    enemy: Option<Vec3>,
    player: bool,
}

/// What a scenario is running.
struct Rig {
    geometry: Geometry,
    world: Option<BrushWorld>,
    anims: AnimTable,
    ps: MotionState,
    time: u64,
    angles: [i16; 3],
    fight: Option<Fight>,
    step: u64,
}

/// The step's keys, from `key=value` words.
#[derive(Debug, Clone, Copy)]
struct Keys {
    command: Command,
    msec: u32,
    turn: [i16; 3],
}

/// Button names (`b=attack+walk`), or a number.
fn button_bits(word: &str) -> Result<u16, String> {
    if let Ok(bits) = word.parse::<u16>() {
        return Ok(bits);
    }
    let mut bits = 0;
    for name in word.split('+') {
        bits |= match name {
            "attack" => buttons::ATTACK,
            "alt" => buttons::ALT_ATTACK,
            "walk" => buttons::WALKING,
            "use" => buttons::USE,
            "useheld" => buttons::USE_HELD,
            "grip" => buttons::FORCE_GRIP,
            "power" => buttons::FORCE_POWER,
            "lightning" => buttons::FORCE_LIGHTNING,
            "drain" => buttons::FORCE_DRAIN,
            "style" => buttons::SABER_STYLE,
            "grenade" => buttons::GRENADE,
            "medpac" => buttons::MEDPAC,
            "shield" => buttons::SHIELD,
            "stim" => buttons::STIM,
            "stealth" => buttons::STEALTH,
            "mine" => buttons::LAY_MINE,
            "" => 0,
            other => return Err(format!("no button {other}")),
        };
    }
    Ok(bits)
}

fn anim(word: &str) -> Result<Anim, String> {
    Anim::from_name(word)
        .or_else(|| word.parse::<u16>().ok().and_then(Anim::from_index))
        .ok_or_else(|| format!("no animation {word}"))
}

fn saber_move(word: &str) -> Result<SaberMove, String> {
    SaberMove::ALL
        .iter()
        .copied()
        .find(|m| m.name().eq_ignore_ascii_case(word))
        .or_else(|| word.parse::<u16>().ok().and_then(SaberMove::from_index))
        .ok_or_else(|| format!("no saber move {word}"))
}

/// Every `Blocked`, in order.
pub fn blocked_all() -> [Blocked; 14] {
    [
        Blocked::None,
        Blocked::BounceMove,
        Blocked::ParryBroken,
        Blocked::AttackBounce,
        Blocked::UpperRight,
        Blocked::UpperLeft,
        Blocked::LowerRight,
        Blocked::LowerLeft,
        Blocked::Top,
        Blocked::UpperRightShot,
        Blocked::UpperLeftShot,
        Blocked::LowerRightShot,
        Blocked::LowerLeftShot,
        Blocked::TopShot,
    ]
}

fn blocked(word: &str) -> Result<Blocked, String> {
    blocked_all()
        .into_iter()
        .find(|b| format!("{b:?}").eq_ignore_ascii_case(word))
        .or_else(|| {
            word.parse::<usize>()
                .ok()
                .and_then(|i| blocked_all().get(i).copied())
        })
        .ok_or_else(|| format!("no blocked {word}"))
}

fn style(word: &str) -> Result<Option<Style>, String> {
    Ok(match word {
        "none" | "-" => None,
        "fast" => Some(Style::Fast),
        "medium" => Some(Style::Medium),
        "strong" => Some(Style::Strong),
        "dual" => Some(Style::Dual),
        "staff" => Some(Style::Staff),
        other => return Err(format!("no style {other}")),
    })
}

fn flag(word: &str) -> Result<bool, String> {
    match word {
        "1" | "true" | "on" => Ok(true),
        "0" | "false" | "off" => Ok(false),
        other => Err(format!("not 0 or 1: {other}")),
    }
}

/// Animation lengths made up from each name: `stand_in`'s one length for
/// all, or `varied`'s lengths (2 to 41 frames, 20 to 99 ms each, some
/// played backward, some without feet, some looping), `long`'s (40 to 100
/// frames, 40 to 90 ms each), `tiny`'s (one or two frames), or
/// `sparse`'s, `varied`'s with about one in nine missing.
fn anims_named(name: &str) -> Result<AnimTable, String> {
    let hashed = |name: &str| {
        let digest = blake3::hash(name.as_bytes());
        u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap_or([0; 8]))
    };
    let varied = |name: &str| {
        let h = hashed(name);
        let lerp = 20 + (h >> 8) % 80;
        let lerp = if (h >> 16) % 11 == 0 {
            -(lerp as i32)
        } else {
            lerp as i32
        };
        let side = 3.0 + ((h >> 32) % 9) as f32;
        let ahead = ((h >> 40) % 9) as f32 - 4.0;
        AnimInfo {
            first_frame: 1 + ((h >> 44) % 900) as i32,
            frames: 2 + (h % 40) as i32,
            frame_lerp: lerp,
            feet: ((h >> 20) % 7 != 0).then_some([[ahead, side], [-ahead, -side]]),
            looping: (h >> 24) % 3 == 0,
        }
    };
    match name {
        "stand_in" => Ok(AnimTable::stand_in()),
        "varied" => Ok(AnimTable::from_lookup(|name| Some(varied(name)))),
        "sparse" => Ok(AnimTable::from_lookup(|name| {
            ((hashed(name) >> 28) % 9 != 0).then(|| varied(name))
        })),
        "long" => Ok(AnimTable::from_lookup(|name| {
            let h = hashed(name);
            Some(AnimInfo {
                frames: 40 + (h % 61) as i32,
                frame_lerp: 40 + ((h >> 8) % 51) as i32,
                ..varied(name)
            })
        })),
        "tiny" => Ok(AnimTable::from_lookup(|name| {
            let h = hashed(name);
            Some(AnimInfo {
                frames: 1 + (h % 2) as i32,
                ..varied(name)
            })
        })),
        other => Err(format!(
            "no anims {other} (stand_in, varied, sparse, long, tiny)"
        )),
    }
}

impl Rig {
    fn new() -> Rig {
        let ps = MotionState::standing([0.0, 0.0, geometry::stand_z(0.0)], 0.0);
        Rig {
            geometry: Geometry::named("flat").unwrap_or_else(|_| Geometry::default()),
            world: None,
            anims: AnimTable::stand_in(),
            angles: ps.command_angles,
            ps,
            time: 0,
            fight: None,
            step: 0,
        }
    }

    fn fight(&mut self) -> Result<&mut Fight, String> {
        self.fight
            .as_mut()
            .ok_or_else(|| "needs `arms` first".to_string())
    }

    /// The keys of a step's words, the last command's angles kept.
    fn keys(&self, words: &[String]) -> Result<Keys, String> {
        let mut keys = Keys {
            command: Command {
                angles: self.angles,
                ..Command::default()
            },
            msec: 8,
            turn: [0; 3],
        };
        let degrees = |v: &str| float(v).map(math::degrees_to_short);
        for (key, value) in pairs(words) {
            let c = &mut keys.command;
            match key {
                "f" => c.forward = int(value)?,
                "r" => c.right = int(value)?,
                "u" => c.up = int(value)?,
                "b" => c.buttons = button_bits(value)?,
                "w" => c.weapon = int(value)?,
                "fs" => c.force = int(value)?,
                "msec" => keys.msec = int(value)?,
                "pitch" => c.angles[0] = degrees(value)?,
                "yaw" => c.angles[1] = degrees(value)?,
                "roll" => c.angles[2] = degrees(value)?,
                "a" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() != 3 {
                        return Err("a=PITCH,YAW,ROLL (shorts)".to_string());
                    }
                    for (axis, part) in parts.iter().enumerate() {
                        c.angles[axis] = int(part)?;
                    }
                }
                "turn" => keys.turn[1] = degrees(value)?,
                "tilt" => keys.turn[0] = degrees(value)?,
                "aim" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() != 3 {
                        return Err("aim=X,Y,Z".to_string());
                    }
                    for (axis, part) in parts.iter().enumerate() {
                        c.aim[axis] = int(part)?;
                    }
                }
                other => return Err(format!("no key {other}")),
            }
        }
        Ok(keys)
    }

    fn world(&mut self) -> &BrushWorld {
        if self.world.is_none() {
            self.world = Some(self.geometry.world());
        }
        self.world.get_or_insert_with(BrushWorld::new)
    }

    /// One `move_body` step (or, `stepped`, a `carry`) with `keys`.
    fn step(
        &mut self,
        keys: &Keys,
        stepped: Option<&crate::walk::Stepped>,
        trace: &mut Trace,
    ) -> Result<(), String> {
        let mut command = keys.command;
        for axis in 0..3 {
            command.angles[axis] = command.angles[axis].wrapping_add(keys.turn[axis]);
        }
        self.angles = command.angles;
        self.time += u64::from(keys.msec);
        self.world();
        let world = self.world.as_ref().ok_or("no world")?;
        let bodies = Bodies::new(world, &self.geometry.bodies);
        let rng_before = self.fight.as_ref().map(|f| f.rng);
        let touches = match &mut self.fight {
            None => match stepped {
                None => motion::move_body(
                    &mut self.ps,
                    &command,
                    keys.msec,
                    self.time,
                    &bodies,
                    &self.anims,
                    None,
                ),
                Some(stepped) => {
                    motion::carry(
                        &mut self.ps,
                        &command,
                        keys.msec,
                        self.time,
                        &bodies,
                        &self.anims,
                        None,
                        stepped,
                    );
                    Vec::new()
                }
            },
            Some(fight) => {
                let mut arms = Arms {
                    fighter: &mut fight.fighter,
                    kit: &fight.kit,
                    player: fight.player,
                    others: &fight.others,
                    rng: &mut fight.rng,
                    enemy: fight.enemy,
                };
                match stepped {
                    None => motion::move_body(
                        &mut self.ps,
                        &command,
                        keys.msec,
                        self.time,
                        &bodies,
                        &self.anims,
                        Some(&mut arms),
                    ),
                    Some(stepped) => {
                        motion::carry(
                            &mut self.ps,
                            &command,
                            keys.msec,
                            self.time,
                            &bodies,
                            &self.anims,
                            Some(&mut arms),
                            stepped,
                        );
                        Vec::new()
                    }
                }
            }
        };
        let mut record = Record::new(self.step, if stepped.is_some() { "carry" } else { "step" });
        record
            .i("dt", keys.msec)
            .i("t", self.time)
            .list("a", &command.angles)
            .i("f", command.forward)
            .i("r", command.right)
            .i("u", command.up)
            .i("b", command.buttons)
            .i("w", command.weapon)
            .i("fs", command.force)
            .list("aim", &command.aim);
        self.outputs(&mut record, rng_before);
        record.list("tch", &touches);
        self.put(record, trace)
    }

    fn put(&mut self, record: Record, trace: &mut Trace) -> Result<(), String> {
        self.step += 1;
        trace.put(&record)
    }

    /// The body's state, and its fight's.
    fn outputs(&self, record: &mut Record, rng_before: Option<SimRng>) {
        player(record, &self.ps);
        if let Some(fight) = &self.fight {
            fighter(record, fight, rng_before);
        }
    }

    /// A directive.
    fn line(&mut self, line: &Line, trace: &mut Trace) -> Result<u64, String> {
        let a = line.args();
        let word = |i: usize| -> Result<&str, String> {
            a.get(i)
                .map(String::as_str)
                .ok_or_else(|| format!("too few words: {}", line.place()))
        };
        match line.verb() {
            "world" => {
                self.geometry = Geometry::named(word(0)?)?;
                self.world = None;
            }
            "box" | "ramp" | "prism" | "body" | "spawn" => {
                self.geometry.add(line)?;
                self.world = None;
            }
            "anims" => self.anims = anims_named(word(0)?)?,
            "start" => {
                let origin = vec3(a)?;
                self.ps = MotionState::standing(origin, float(word(3)?)?);
                self.angles = self.ps.command_angles;
            }
            "time" => self.time = int(word(0)?)?,
            "set" => self.set(a)?,
            "arms" => self.arms(a)?,
            "rng" => {
                let (seed, stream) = (int(word(0)?)?, int(word(1)?)?);
                self.fight()?.rng = SimRng::new(seed, stream);
            }
            "other" => {
                let (entity, legs) = (int(word(0)?)?, anim(word(1)?)?);
                let fight = self.fight()?;
                fight.others.push(Other { entity, legs });
                fight.others.sort_by_key(|other| other.entity);
            }
            "enemy" => {
                let enemy = if word(0)? == "none" {
                    None
                } else {
                    Some(vec3(a)?)
                };
                self.fight()?.enemy = enemy;
            }
            "saber" => self.saber(a)?,
            "force" => self.force(a)?,
            "knockdown" => {
                let (now, ms) = (self.time, int(word(0)?)?);
                self.fight()?.fighter.saber.knock_down(now, ms);
            }
            "standup" => self.fight()?.fighter.saber.stand_up(),
            "step" => {
                let (count, rest) = match a.first().and_then(|w| w.parse::<u64>().ok()) {
                    Some(count) => (count, &a[1..]),
                    None => (1, a),
                };
                let keys = self.keys(rest)?;
                for _ in 0..count {
                    self.step(&keys, None, trace)?;
                }
                return Ok(count);
            }
            "carry" => {
                let stepped = crate::walk::Stepped {
                    origin: vec3(a)?,
                    velocity: vec3(a.get(3..).unwrap_or(&[]))?,
                    yaw: float(word(6)?)?,
                    legs: anim(word(7)?)?,
                    bumped: None,
                    door: None,
                    pushed: None,
                };
                let keys = self.keys(a.get(8..).unwrap_or(&[]))?;
                self.step(&keys, Some(&stepped), trace)?;
                return Ok(1);
            }
            "react" | "hold" | "start_torso" => {
                let which = anim(word(0)?)?;
                match line.verb() {
                    "react" => motion::react(&mut self.ps, &self.anims, which, word(1)? == "torso"),
                    "hold" => {
                        let msec = a.get(2).map(|w| int(w)).transpose()?.unwrap_or(8);
                        motion::hold(&mut self.ps, &self.anims, which, word(1)? == "both", msec);
                    }
                    _ => {
                        let sent = anim(word(1)?)?;
                        motion::begin_torso(&mut self.ps, &self.anims, which, sent);
                    }
                }
                let mut record = Record::new(self.step, line.verb());
                record.s("anim", which.name());
                self.outputs(&mut record, None);
                self.put(record, trace)?;
                return Ok(1);
            }
            "query" => {
                let mut record = Record::new(self.step, "query");
                match word(0)? {
                    "stopping_distance" => {
                        let speed = float(word(1)?)?;
                        record
                            .f("speed", speed)
                            .f("distance", motion::stopping_distance(speed));
                    }
                    "view_of" => {
                        let mut angles = [0i16; 3];
                        let mut delta = [0i16; 3];
                        for axis in 0..3 {
                            angles[axis] = int(word(1 + axis)?)?;
                            delta[axis] = int(word(4 + axis)?)?;
                        }
                        record
                            .list("angles", &angles)
                            .list("delta", &delta)
                            .v3("view", motion::view_of(angles, delta));
                    }
                    "anims_digest" => {
                        let table = self.anims.clone().without_skeleton();
                        record.hex("digest", &table.digest());
                        // With a skeleton: the least one there is.
                        let table = table.with_skeleton(std::sync::Arc::new(least_skeleton()?));
                        record
                            .b("skeleton", table.skeleton().is_some())
                            .hex("digest_skeleton", &table.digest());
                    }
                    "shrugs_off_pain" => {
                        record.b("shrugs", motion::shrugs_off_pain(&self.ps));
                    }
                    other => return Err(format!("no query {other}")),
                }
                self.put(record, trace)?;
                return Ok(1);
            }
            other => return Err(format!("no directive {other}: {}", line.place())),
        }
        Ok(0)
    }

    /// `set FIELD VALUES`.
    fn set(&mut self, a: &[String]) -> Result<(), String> {
        let field = a.first().map(String::as_str).unwrap_or("");
        let rest = a.get(1..).unwrap_or(&[]);
        let first = || -> Result<&str, String> {
            rest.first()
                .map(String::as_str)
                .ok_or_else(|| format!("set {field}: needs a value"))
        };
        let ps = &mut self.ps;
        match field {
            "origin" => ps.origin = vec3(rest)?,
            "velocity" => ps.velocity = vec3(rest)?,
            "view" => ps.set_view(vec3(rest)?),
            "face" => ps.face(float(first()?)?),
            "delta" => {
                for axis in 0..3 {
                    ps.view_offset[axis] = int(rest.get(axis).ok_or("delta: three")?)?;
                }
            }
            "flags" => ps.flags = int(first()?)?,
            "pm_time" => ps.hold_ms = int(first()?)?,
            "legs" | "torso" => {
                let part = if field == "legs" {
                    &mut ps.legs
                } else {
                    &mut ps.torso
                };
                part.anim = anim(first()?)?;
                part.timer = int(rest.get(1).ok_or("needs a timer")?)?;
            }
            "ground" => {
                ps.ground = match first()? {
                    "none" => None,
                    "world" => Some(Ground::World),
                    entity => Some(Ground::Entity(int(entity)?)),
                }
            }
            "gravity" => ps.gravity = float(first()?)?,
            "base_speed" => ps.run_speed = float(first()?)?,
            "maxs_z" => ps.maxs_z = float(first()?)?,
            "view_height" => ps.eye_height = float(first()?)?,
            "jump_start_z" => ps.takeoff_z = float(first()?)?,
            "movement_dir" => ps.push_dir = int(first()?)?,
            "in_air" => ps.in_air_anim = flag(first()?)?,
            "bob" => ps.footstep_cycle = int(first()?)?,
            "slope_recalc" => ps.slope_next_ms = int(first()?)?,
            "use_delay" => ps.next_use_ms = int(first()?)?,
            "fp" => self.fight()?.fighter.fp = int(first()?)?,
            "hp" => self.fight()?.fighter.hp = int(first()?)?,
            "weapon_ms" => self.fight()?.fighter.weapon_wait_ms = int(first()?)?,
            "hand" => self.fight()?.fighter.hand = int(first()?)?,
            "combat_until" => self.fight()?.fighter.combat_until = int(first()?)?,
            "target" => {
                self.fight()?.fighter.target = match first()? {
                    "none" => None,
                    id => Some(SimId(int(id)?)),
                }
            }
            "sheath" => {
                self.fight()?.fighter.sheath = match first()? {
                    "fight" => Sheath::Fight,
                    "holstered" => Sheath::Holstered,
                    "lit" => Sheath::Lit,
                    "holstering" => Sheath::Holstering,
                    other => return Err(format!("no sheath {other}")),
                }
            }
            other => return Err(format!("no field {other}")),
        }
        Ok(())
    }

    /// `arms KIND [key=value]`: what the hands hold, the feats' tiers and
    /// the pool.
    fn arms(&mut self, a: &[String]) -> Result<(), String> {
        let kind = a.first().map(String::as_str).unwrap_or("none");
        let mut blade = Blade {
            saber: true,
            double: false,
            proficient: true,
        };
        let mut kit = Kit::default();
        let (mut max_hp, mut player) = (60u16, true);
        for (key, value) in pairs(a.get(1..).unwrap_or(&[])) {
            match key {
                "jump" => kit.force_jump = int(value)?,
                "fp" => {
                    kit.max_fp = int(value)?;
                    kit.pool = kit.max_fp;
                }
                "pool" => kit.pool = int(value)?,
                "hp" => max_hp = int(value)?,
                "flurry" => kit.flurry = int(value)?,
                "critical" => kit.critical = int(value)?,
                "power" => kit.power = int(value)?,
                "two_weapon" => kit.two_weapon = int(value)?,
                "defense" => kit.jedi_defense = int(value)?,
                "dueling" => kit.dueling = int(value)?,
                "proficient" => blade.proficient = flag(value)?,
                "melee" => blade.saber = !flag(value)?,
                "player" => player = flag(value)?,
                "throw" => {
                    // `LEVEL` or `LEVEL:COST` (hundredths of the pool).
                    let (level, cost) = value.split_once(':').unwrap_or((value, "0"));
                    kit.throw = Some(Throw {
                        level: int(level)?,
                        row: 0,
                        cost: int(cost)?,
                    });
                }
                other => return Err(format!("arms: no key {other}")),
            }
        }
        kit.hands = match kind {
            "none" => {
                self.fight = None;
                return Ok(());
            }
            "fists" => [Held::Other, Held::Nothing],
            "saber" => [Held::Blade(blade), Held::Nothing],
            "dual" => [Held::Blade(blade), Held::Blade(blade)],
            "staff" => [
                Held::Blade(Blade {
                    double: true,
                    ..blade
                }),
                Held::Nothing,
            ],
            "saber_fists" => [Held::Blade(blade), Held::Other],
            "pistol" => [Held::Blaster(Blaster::Pistol), Held::Nothing],
            "rifle" => [Held::Blaster(Blaster::Rifle), Held::Nothing],
            "pistols" => [
                Held::Blaster(Blaster::Pistol),
                Held::Blaster(Blaster::Pistol),
            ],
            "rifle_saber" => [Held::Blaster(Blaster::Rifle), Held::Blade(blade)],
            "saber_pistol" => [Held::Blade(blade), Held::Blaster(Blaster::Pistol)],
            other => return Err(format!("no arms {other}")),
        };
        self.fight = Some(Fight {
            fighter: Fighter::fresh(max_hp, kit.pool),
            kit,
            rng: SimRng::new(24, 3),
            others: Vec::new(),
            enemy: None,
            player,
        });
        Ok(())
    }

    /// `saber key=value`: the saber's state as the server leaves it.
    fn saber(&mut self, a: &[String]) -> Result<(), String> {
        let saber = &mut self.fight()?.fighter.saber;
        for (key, value) in pairs(a) {
            match key {
                "on" => saber.on = flag(value)?,
                "second_off" => saber.second_off = flag(value)?,
                "style" => saber.style = style(value)?,
                "queued" => saber.queued = style(value)?,
                "move" => saber.current_move = saber_move(value)?,
                "blocked" => saber.blocked = blocked(value)?,
                "blocking" => {
                    saber.blocking = match value {
                        "no" => Block::No,
                        "tight" => Block::Tight,
                        "wide" => Block::Wide,
                        other => return Err(format!("no block {other}")),
                    }
                }
                "chain" => saber.chain = int(value)?,
                "change" => {
                    saber.change = match value {
                        "none" => Change::None,
                        "dropping" => Change::Dropping,
                        "raising" => Change::Raising,
                        other => return Err(format!("no change {other}")),
                    }
                }
                "hand" => {
                    saber.hand = match value {
                        "none" => Hand::None,
                        "knockdown" => Hand::Knockdown,
                        "ready" => Hand::Risen,
                        other => return Err(format!("no hand {other}")),
                    }
                }
                "hand_until" => saber.hand_until = int(value)?,
                "getup" => {
                    saber.getup = match value {
                        "down" => Getup::Down,
                        "plain" => Getup::Plain,
                        "force" => Getup::Force,
                        "quick" => Getup::Quick,
                        other => return Err(format!("no getup {other}")),
                    }
                }
                "quicker" => saber.quicker = flag(value)?,
                "lock_until" => saber.lock_until = int(value)?,
                "cycle_held" => saber.cycle_held = flag(value)?,
                "out_until" => saber.out_until = int(value)?,
                "thrown" => {
                    let flight = match value {
                        "none" => None,
                        "leaving" => Some(Flight::Leaving),
                        "out" => Some(Flight::Out),
                        "back" => Some(Flight::Back),
                        "falling" => Some(Flight::Falling),
                        "lying" => Some(Flight::Lying),
                        other => return Err(format!("no flight {other}")),
                    };
                    saber.thrown = flight.map(|flight| Thrown {
                        flight,
                        ..Thrown::leaving(1, 0)
                    });
                }
                other => return Err(format!("saber: no key {other}")),
            }
        }
        Ok(())
    }

    /// `force key=value`: the Force state a step reads.
    fn force(&mut self, a: &[String]) -> Result<(), String> {
        let force = &mut self.fight()?.fighter.force;
        for (key, value) in pairs(a) {
            match key {
                "jumping" => force.jumping = flag(value)?,
                "jump_paid_at" => force.jump_paid_at = int(value)?,
                "stunned_until" => force.stunned_until = int(value)?,
                "hand_until" => {
                    force.hand = Some((crate::combat::force::Gesture::Push, int(value)?));
                }
                "gripped_by" => {
                    force.gripped_by = match value {
                        "none" => None,
                        id => Some(SimId(int(id)?)),
                    }
                }
                other => return Err(format!("force: no key {other}")),
            }
        }
        Ok(())
    }
}

/// A skeleton of two bones (the root and the torso's), one frame, both
/// hands' tags on the torso bone, no sabers.
fn least_skeleton() -> Result<crate::skeleton::SkeletonData, String> {
    use crate::skeleton::{Bone, SkeletonData, SkeletonParts, Tag, TagVertex};
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let bone = |name: &str, parent| Bone {
        name: name.to_string(),
        parent,
        base: identity,
        base_inverse: identity,
    };
    let vertex = |x: f32| TagVertex {
        position: [x, 1.0, 2.0],
        influences: vec![(1, 1023)],
    };
    let tag = || Tag {
        vertices: [vertex(0.0), vertex(1.0), vertex(2.0)],
    };
    SkeletonData::new(SkeletonParts {
        bones: vec![bone("model_root", None), bone("lower_lumbar", Some(0))],
        frame_count: 1,
        keys: vec![0, 0],
        pool: vec![[0, 0, 0, 0, 0, 0, 0]],
        hands: [tag(), tag()],
        sabers: Vec::new(),
        motion: None,
    })
    .map_err(|e| e.to_string())
}

/// The body's state, every field.
fn player(record: &mut Record, ps: &MotionState) {
    let ground = match ps.ground {
        None => "-".to_string(),
        Some(Ground::World) => "W".to_string(),
        Some(Ground::Entity(entity)) => format!("E{entity}"),
    };
    record
        .v3("o", ps.origin)
        .v3("v", ps.velocity)
        .v3("view", ps.view)
        .list("da", &ps.view_offset)
        .s("gr", &ground)
        .i("fl", ps.flags)
        .i("pmt", ps.hold_ms)
        .s("legs", ps.legs.anim.name())
        .i("lt", ps.legs.timer)
        .b("lf", ps.legs.flip)
        .s("torso", ps.torso.anim.name())
        .i("tt", ps.torso.timer)
        .b("tf", ps.torso.flip)
        .i("md", ps.push_dir)
        .b("air", ps.in_air_anim)
        .i("bob", ps.footstep_cycle)
        .f("mz", ps.maxs_z)
        .f("vh", ps.eye_height)
        .f("jz", ps.takeoff_z)
        .i("srm", ps.slope_next_ms)
        .f("bs", ps.run_speed)
        .f("g", ps.gravity)
        .i("ev", ps.events)
        .list("ca", &ps.command_angles)
        .list("caim", &ps.command_aim)
        .i("ud", ps.next_use_ms);
}

/// The fight's state: the saber's fields, the pool, the weapon timer, the
/// Force jump's, the stream and the draws the step took, and a digest of
/// the fighter's whole encoding.
fn fighter(record: &mut Record, fight: &Fight, rng_before: Option<SimRng>) {
    let f = &fight.fighter;
    let s = &f.saber;
    record
        .i("hp", f.hp)
        .i("fp", f.fp)
        .i("wms", f.weapon_wait_ms)
        .i("hand", f.hand)
        .d("sheath", &f.sheath)
        .i("cu", f.combat_until)
        .b("on", s.on)
        .b("off2", s.second_off)
        .s("sty", s.style.map_or("-", Style::name))
        .s("q", s.queued.map_or("-", Style::name))
        .s("mv", s.current_move.name())
        .d("blk", &s.blocking)
        .d("bkd", &s.blocked)
        .i("chn", s.chain)
        .d("chg", &s.change)
        .d("hnd", &s.hand)
        .i("hu", s.hand_until)
        .d("gu", &s.getup)
        .b("qk", s.quicker)
        .i("lku", s.lock_until)
        .b("cyc", s.cycle_held)
        .i("out", s.out_until)
        .d("thr", &s.thrown.map(|t| t.flight))
        .b("fj", f.force.jumping)
        .i("fjp", f.force.jump_paid_at);
    let mut state = Writer::default();
    fight.rng.encode(&mut state);
    record.hex("rng", &state.0);
    if let Some(before) = rng_before {
        record.i("draws", draws(before, fight.rng));
    }
    let mut encoded = Writer::default();
    f.encode(&mut encoded);
    record.hex("fenc", &blake3::hash(&encoded.0).as_bytes()[..8]);
}

/// How many numbers the stream drew from `before` to `after` (`>1024`
/// past that).
fn draws(before: SimRng, after: SimRng) -> String {
    let mut rng = before;
    for count in 0..=1024 {
        if rng == after {
            return count.to_string();
        }
        rng.next_u32();
    }
    ">1024".to_string()
}

/// Runs a scenario; the steps it recorded.
pub fn run(scenario: &Scenario, trace: &mut Trace) -> Result<u64, String> {
    let mut rig = Rig::new();
    let mut steps = 0;
    for line in &scenario.lines {
        steps += rig
            .line(line, trace)
            .map_err(|e| format!("{e} ({})", line.place()))?;
    }
    Ok(steps)
}

/// The generator's draws (its own stream, not the sim's).
pub(super) struct Gen(SimRng);

impl Gen {
    pub fn new(seed: u64, salt: u64) -> Gen {
        Gen(SimRng::new(seed, salt))
    }

    pub fn below(&mut self, n: u32) -> u32 {
        self.0.below(n.max(1))
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.0.unit() < p
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        self.0.range(low, high)
    }

    pub fn int(&mut self, low: i64, high: i64) -> i64 {
        low + i64::from(self.below((high - low + 1) as u32))
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u32) as usize]
    }

    pub fn u32(&mut self) -> u32 {
        self.0.next_u32()
    }
}

/// An axis's value: still, full either way, half, or anything.
fn axis(g: &mut Gen) -> i8 {
    match g.below(10) {
        0..=2 => 0,
        3 | 4 => 127,
        5 => -127,
        6 => g.pick(&[64, -64, 30, -30, 1, -1]),
        7 => -128,
        _ => g.int(-128, 127) as i8,
    }
}

/// A step's milliseconds: JA's 8 mostly, else any, or the extremes the
/// step clamps.
fn msec(g: &mut Gen) -> u32 {
    match g.below(100) {
        0..=84 => 8,
        85..=92 => g.int(1, 50) as u32,
        93..=96 => g.int(1, 200) as u32,
        _ => g.pick(&[0, 1, 2, 199, 200, 201, 255, 1000, 65_535]),
    }
}

/// `m1`'s (`armed`: `m3`'s) random scenarios for `seed`: episodes of up
/// to 600 steps, `n` steps in all, each in a world of the library from a
/// place it lists, with random keys held for random spells, and now and
/// then a push, a knockback, a reaction or a walker's carry between them.
pub fn random(seed: u64, n: u64, armed: bool) -> Vec<Scenario> {
    let mut g = Gen::new(seed, if armed { 0x6d33 } else { 0x6d31 });
    let mut out = Vec::new();
    let mut left = n;
    let mut episode = 0;
    while left > 0 {
        let steps = left.min(u64::from(g.int(100, 600) as u32));
        out.push(episode_of(
            &mut g,
            format!("random-{seed}-{episode}"),
            steps,
            armed,
        ));
        left -= steps;
        episode += 1;
    }
    out
}

fn words(items: &[String]) -> Vec<String> {
    items.to_vec()
}

fn episode_of(g: &mut Gen, name: String, steps: u64, armed: bool) -> Scenario {
    let mut s = Scenario::new(name);
    let world = g.pick(geometry::NAMES);
    s.push(&["world", world]);
    let geometry = Geometry::named(world).unwrap_or_else(|_| Geometry::default());
    s.push(&[
        "anims",
        g.pick(&["stand_in", "stand_in", "varied", "sparse"]),
    ]);
    let spawn = if geometry.spawns.is_empty() {
        ([0.0; 3], [0.0; 3])
    } else {
        geometry.spawns[g.below(geometry.spawns.len() as u32) as usize]
    };
    let at: Vec<f32> = (0..3)
        .map(|i| {
            if spawn.0[i] == spawn.1[i] {
                spawn.0[i]
            } else {
                g.range(spawn.0[i], spawn.1[i])
            }
        })
        .collect();
    let yaw = g.range(-180.0, 180.0);
    s.push_owned(words(&[
        "start".into(),
        at[0].to_string(),
        at[1].to_string(),
        at[2].to_string(),
        yaw.to_string(),
    ]));
    s.push_owned(vec!["time".into(), g.int(0, 100_000).to_string()]);
    // `m1` too fights now and then: its moves change the command, the
    // speed and the legs, and hold the view.
    let armed = armed || g.chance(0.15);
    let fists = !armed && g.chance(0.45);
    if armed {
        arms_armed(g, &mut s, [at[0], at[1], at[2]]);
    } else if fists {
        let jump = g.int(0, 3);
        let fp = g.pick(&[0, 0, 20, 100, 250]);
        s.push_owned(vec![
            "arms".into(),
            "fists".into(),
            format!("jump={jump}"),
            format!("fp={fp}"),
        ]);
        s.push_owned(vec!["rng".into(), g.u32().to_string(), g.u32().to_string()]);
    }
    if g.chance(0.2) {
        let v = [
            g.range(-600.0, 600.0),
            g.range(-600.0, 600.0),
            g.range(-400.0, 700.0),
        ];
        s.push_owned(set3("velocity", v));
    }
    if g.chance(0.05) {
        let gravity = g.pick(&[0.0, 200.0, 400.0, 1600.0]);
        s.push_owned(vec!["set".into(), "gravity".into(), gravity.to_string()]);
    }
    if g.chance(0.05) {
        let speed = g.pick(&[0.0, 50.0, 125.0, 500.0]);
        s.push_owned(vec!["set".into(), "base_speed".into(), speed.to_string()]);
    }
    if g.chance(0.03) {
        s.push_owned(vec![
            "set".into(),
            "flags".into(),
            motion::motion_flags::DEAD.to_string(),
        ]);
    }
    if g.chance(0.04) {
        s.push_owned(vec![
            "set".into(),
            "flags".into(),
            motion::motion_flags::PACED.to_string(),
        ]);
    }
    let mut left = steps;
    let mut aim = [0i16; 3];
    aim[1] = math::degrees_to_short(yaw);
    while left > 0 {
        if g.chance(0.08) {
            extra(g, &mut s, armed || fists);
        }
        let spell = left.min(u64::from(g.int(1, 80) as u32));
        let mut keys: Vec<String> = vec!["step".into(), spell.to_string()];
        let (f, r) = (axis(g), axis(g));
        if f != 0 {
            keys.push(format!("f={f}"));
        }
        if r != 0 {
            keys.push(format!("r={r}"));
        }
        let up = match g.below(20) {
            0..=12 => 0,
            13..=15 => 127,
            16 | 17 => -127,
            18 => g.pick(&[10, -10, 1, -1]),
            _ => g.int(-128, 127) as i8,
        };
        if up != 0 {
            keys.push(format!("u={up}"));
        }
        let mut bits = 0u16;
        if g.chance(0.15) {
            bits |= buttons::WALKING;
        }
        if armed {
            bits |= armed_buttons(g);
        } else if g.chance(0.03) {
            bits |= g.u32() as u16;
        }
        if bits != 0 {
            keys.push(format!("b={bits}"));
        }
        if armed && g.chance(0.05) {
            keys.push(format!("w={}", g.below(2)));
        }
        match g.below(10) {
            0..=5 => {}
            6..=8 => keys.push(format!("turn={}", g.range(-6.0, 6.0))),
            _ => keys.push(format!("turn={}", g.range(-180.0, 180.0))),
        }
        if g.chance(0.15) {
            aim[0] = g.int(-20_000, 20_000) as i16;
            aim[1] = g.u32() as i16;
            if g.chance(0.1) {
                aim[2] = g.u32() as i16;
            }
            keys.push(format!("a={},{},{}", aim[0], aim[1], aim[2]));
        }
        let dt = msec(g);
        if dt != 8 {
            keys.push(format!("msec={dt}"));
        }
        s.push_owned(keys);
        left -= spell;
    }
    s
}

fn set3(field: &str, v: [f32; 3]) -> Vec<String> {
    vec![
        "set".into(),
        field.into(),
        v[0].to_string(),
        v[1].to_string(),
        v[2].to_string(),
    ]
}

/// Something between spells of keys: a push, a knockback, a pose, a
/// teleport within reach, a walker's step, a query.
fn extra(g: &mut Gen, s: &mut Scenario, armed: bool) {
    let random_anim = |g: &mut Gen| Anim::ALL[g.below(Anim::ALL.len() as u32) as usize].name();
    match g.below(if armed { 12 } else { 10 }) {
        0 => {
            let v = [
                g.range(-900.0, 900.0),
                g.range(-900.0, 900.0),
                g.range(-300.0, 900.0),
            ];
            s.push_owned(set3("velocity", v));
        }
        1 => {
            s.push_owned(vec![
                "set".into(),
                "flags".into(),
                motion::motion_flags::KNOCKBACK_HOLD.to_string(),
            ]);
            s.push_owned(vec![
                "set".into(),
                "pm_time".into(),
                g.int(1, 900).to_string(),
            ]);
        }
        2 => {
            let which = if g.chance(0.5) { "torso" } else { "both" };
            s.push_owned(vec!["react".into(), random_anim(g).into(), which.into()]);
        }
        3 => {
            let which = if g.chance(0.5) { "torso" } else { "both" };
            s.push_owned(vec![
                "hold".into(),
                random_anim(g).into(),
                which.into(),
                g.int(0, 60).to_string(),
            ]);
        }
        4 => s.push_owned(vec![
            "start_torso".into(),
            random_anim(g).into(),
            random_anim(g).into(),
        ]),
        5 => {
            let legs = g.pick(&["BOTH_WALK1", "BOTH_RUN1", "BOTH_STAND1", "BOTH_WALKBACK1"]);
            s.push_owned(vec![
                "carry".into(),
                g.range(-512.0, 512.0).to_string(),
                g.range(-512.0, 512.0).to_string(),
                geometry::stand_z(0.0).to_string(),
                g.range(-300.0, 300.0).to_string(),
                g.range(-300.0, 300.0).to_string(),
                "0".into(),
                g.range(-180.0, 180.0).to_string(),
                legs.into(),
                format!("f={}", axis(g)),
                format!("u={}", axis(g)),
            ]);
        }
        6 => {
            let roll = g.pick(&["BOTH_ROLL_F", "BOTH_ROLL_B", "BOTH_ROLL_L", "BOTH_ROLL_R"]);
            s.push_owned(vec![
                "set".into(),
                "legs".into(),
                roll.into(),
                g.int(0, 900).to_string(),
            ]);
        }
        7 => s.push_owned(vec![
            "query".into(),
            "stopping_distance".into(),
            g.range(0.0, 1200.0).to_string(),
        ]),
        8 => s.push_owned(vec![
            "query".into(),
            "view_of".into(),
            (g.u32() as i16).to_string(),
            (g.u32() as i16).to_string(),
            (g.u32() as i16).to_string(),
            (g.u32() as i16).to_string(),
            (g.u32() as i16).to_string(),
            (g.u32() as i16).to_string(),
        ]),
        9 => {
            let flags = g.u32() & 0x7ff;
            s.push_owned(vec!["set".into(), "flags".into(), flags.to_string()]);
        }
        10 => {
            let mv = SaberMove::ALL[g.below(SaberMove::ALL.len() as u32) as usize].name();
            let blocked = blocked_all()[g.below(14) as usize];
            s.push_owned(vec![
                "saber".into(),
                format!("blocked={blocked:?}"),
                if g.chance(0.5) {
                    format!("move={mv}")
                } else {
                    "on=1".into()
                },
            ]);
        }
        _ => match g.below(5) {
            3 => {
                let sheath = g.pick(&["fight", "holstered", "lit", "holstering"]);
                s.push_owned(vec!["set".into(), "sheath".into(), sheath.into()]);
            }
            4 => {
                s.push_owned(vec![
                    "set".into(),
                    "combat_until".into(),
                    g.int(0, 200_000).to_string(),
                ]);
                let target = if g.chance(0.5) {
                    "none".to_string()
                } else {
                    g.int(1, 9).to_string()
                };
                s.push_owned(vec!["set".into(), "target".into(), target]);
            }
            0 => s.push_owned(vec!["knockdown".into(), g.int(100, 3000).to_string()]),
            1 => s.push_owned(vec![
                "saber".into(),
                format!("lock_until={}", g.int(0, 200_000)),
            ]),
            2 if g.chance(0.6) => {
                let flight = g.pick(&["none", "leaving", "out", "back", "falling", "lying"]);
                s.push_owned(vec!["saber".into(), format!("thrown={flight}")]);
            }
            _ => s.push_owned(vec!["query".into(), "shrugs_off_pain".into()]),
        },
    }
}

/// A fight for `m3`: a saber, two or a staff (or fists), feats' tiers at
/// random, a pool, a stream, and now and then others standing by.
fn arms_armed(g: &mut Gen, s: &mut Scenario, at: [f32; 3]) {
    let kind = g.pick(&[
        "saber",
        "saber",
        "saber",
        "dual",
        "dual",
        "staff",
        "staff",
        "fists",
        "saber_fists",
        "pistol",
        "rifle",
        "pistols",
        "rifle_saber",
        "saber_pistol",
    ]);
    let mut line: Vec<String> = vec!["arms".into(), kind.into()];
    for key in [
        "flurry",
        "critical",
        "power",
        "two_weapon",
        "defense",
        "jump",
    ] {
        line.push(format!("{key}={}", g.int(0, 3)));
    }
    line.push(format!("dueling={}", g.int(0, 1)));
    line.push(format!("fp={}", g.pick(&[0, 20, 100, 250])));
    if g.chance(0.1) {
        line.push("proficient=0".into());
    }
    if g.chance(0.1) {
        line.push("melee=1".into());
    }
    if g.chance(0.2) {
        line.push("player=0".into());
    }
    if g.chance(0.4) {
        line.push(format!(
            "throw={}:{}",
            g.int(1, 2),
            g.pick(&[0, 0, 10, 50, 500])
        ));
    }
    s.push_owned(line);
    s.push_owned(vec!["rng".into(), g.u32().to_string(), g.u32().to_string()]);
    if g.chance(0.4) {
        s.push_owned(vec![
            "enemy".into(),
            (at[0] + g.range(-200.0, 200.0)).to_string(),
            (at[1] + g.range(-200.0, 200.0)).to_string(),
            (at[2] + g.range(-40.0, 100.0)).to_string(),
        ]);
    }
    // Others standing by: up and down, ahead, behind, to the sides.
    if g.chance(0.4) {
        for k in 0..g.int(1, 4) {
            let entity = (k as u32 + 1) | crate::BODY;
            let free = g.range(0.0, 360.0);
            let yaw = g.pick(&[0.0f32, 90.0, 180.0, 270.0, free]).to_radians();
            let reach = g.range(30.0, 150.0);
            let (x, y) = (at[0] + reach * yaw.cos(), at[1] + reach * yaw.sin());
            let half = g.range(10.0, 16.0);
            let z = at[2] - 24.0;
            s.push_owned(vec![
                "body".into(),
                entity.to_string(),
                (x - half).to_string(),
                (y - half).to_string(),
                z.to_string(),
                (x + half).to_string(),
                (y + half).to_string(),
                (z + g.pick(&[24.0, 64.0])).to_string(),
            ]);
            let legs = g.pick(&[
                "BOTH_STAND1",
                "BOTH_KNOCKDOWN1",
                "BOTH_KNOCKDOWN4",
                "BOTH_RUN1",
            ]);
            s.push_owned(vec!["other".into(), entity.to_string(), legs.into()]);
        }
    }
}

/// Buttons a saber fight presses: attack, alt, the style, both.
fn armed_buttons(g: &mut Gen) -> u16 {
    let mut bits = 0;
    if g.chance(0.45) {
        bits |= buttons::ATTACK;
    }
    if g.chance(0.15) {
        bits |= buttons::ALT_ATTACK;
    }
    if g.chance(0.06) {
        bits |= buttons::SABER_STYLE;
    }
    if g.chance(0.02) {
        bits |= buttons::USE;
    }
    bits
}
