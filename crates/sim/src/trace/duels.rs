//! `m6`: the world's blades (`world::sabers`) driven through `SimWorld`:
//! players on the arena's floor with friendly fire full, stepped on the
//! authority with scripted keys, each looking at the other's chest. Their
//! skeleton is one made here (`duel_skeleton`: a torso that turns through
//! its frames, a hand on it, a blade and a staff's two), so blades sweep,
//! meet, lock and cut without game data; each animation plays a run of its
//! frames (`duel_anims`).
//!
//! Directives, in order (`docs/EQUIVALENCE.md`):
//! - before `begin`: `seed N`, `friendly off|area|full`, `anims
//!   full|sparse|without TEXT` (about one animation in nine missing, or
//!   every one whose name holds `TEXT`), `module
//!   COST|none` (a module with nothing placed, every power costing COST:
//!   the Force needs one), `fighter SEAT
//!   KIND [key=value]` (`KIND` one of `fast medium strong dual staff
//!   pistol rifle ion fists`, `ion` a blaster whose bolts no blade turns; keys `x y z yaw defence hp fp throw jump tiers
//!   proficient powers`, `tiers` the feat lines' tiers, 0 to 3, `powers`
//!   the `spells.2da` rows known, joined by commas);
//!   `creature KIND [faction=hostile|friendly|ally] [keys]` (a creature placed in
//!   the module, a module made if none was asked for, fighting by its own
//!   mind; recorded as `c<N>`);
//! - `begin`: the world is made with its fighters where they were put;
//! - `tick [N] [key=value]`: N ticks of commands, keys by seat (`f0 r0 u0
//!   b0 t0 w0 p0`, `f1 ...`: the axes, the buttons as names joined by `+`
//!   (`attack alt walk use style force grip lightning drain`), the degrees
//!   turned from looking at the other, the weapon hand, the power row the
//!   force button casts);
//! - `until SEAT locked|unlocked|down|gripped N [key=value]`: up to N ticks, ending
//!   on the first after which the seat is so; `lock ATT DEF KIND` a lock of
//!   `top diag_tr diag_tl diag_br diag_bl r l` started between two seats;
//! - `lag SEAT MS`, `wound SEAT HP`, `saber SEAT key=value` (`on style
//!   move lock_until lock=none thrown`), `place SEAT X Y Z YAW`: changed in the
//!   world's snapshot, the world made again from it; so too `put WHO X Y Z
//!   YAW`, `toggle WHO absorb|protect LEVEL MS`, `stat WHO hp|fp N`, `force
//!   WHO key=value` (`hand throw_ready grip_ready drain_ready stunned
//!   immune`, ms from now), `knockback WHO MS`, `pose WHO ANIM MS` (the
//!   torso), `away WHO FLIGHT LEVEL` (its saber thrown), `shift WHO DX DY DZ`
//!   (moved, all else kept), `roundtrip` (the snapshot through its bytes),
//!   `WHO` a seat (`0`)
//!   or a creature (`c0`); `reason predict|authority` (how the ticks after
//!   step); tick keys also `l0` (degrees looked down, up negative) and `c0=N`
//!   (look at creature N); fighters also `plot=1`.

use std::sync::Arc;

use super::movement::Gen;
use super::record::{Record, Trace};
use super::scenario::{Scenario, float, int, pairs};
use crate::combat::Fighter;
use crate::combat::damage::FriendlyFire;
use crate::combat::mind;
use crate::combat::saber::throw::Flight;
use crate::combat::saber::{Move, Style, Thrown};
use crate::combat::sheet::{Sheet, Weapon, damage_types, feats};
use crate::motion::{self, AnimInfo, AnimTable, MotionState};
use crate::skeleton::{Bone, Saber, SaberBlade, SkeletonData, SkeletonParts, Tag, TagVertex};
use crate::{
    Command, Kind, Level, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, WorldConfig,
    buttons,
};

/// Frames of the made-up skeleton's one animation track.
fn frame_total() -> u32 {
    64
}

/// A bone's compressed transform: a turn of `degrees` about `axis` (made
/// unit), then `t` (units).
fn key(axis: [f32; 3], degrees: f32, t: [f32; 3]) -> [u16; 7] {
    let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let half = degrees.to_radians() * 0.5;
    let (s, c) = (half.sin() / length, half.cos());
    let q = |v: f32| ((v + 2.0) * 16383.0).round() as u16;
    let p = |v: f32| ((v + 512.0) * 64.0).round() as u16;
    [
        q(c),
        q(axis[0] * s),
        q(axis[1] * s),
        q(axis[2] * s),
        p(t[0]),
        p(t[1]),
        p(t[2]),
    ]
}

/// The skeleton the duels pose: a root, the torso's lumbar bones and
/// thoracic (the look's), a `Motion` bone; the torso turns about a slanted
/// axis back and forth across the front through its frames, the thoracic
/// sways, so a blade in a
/// hand on it sweeps as an animation plays. A single saber's blade and a
/// staff's two point along the hand's first axis, each way.
fn duel_skeleton() -> Result<SkeletonData, String> {
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
    let bones = vec![
        bone("model_root", None),
        bone("lower_lumbar", Some(0)),
        bone("upper_lumbar", Some(1)),
        bone("thoracic", Some(2)),
        bone("Motion", Some(0)),
    ];
    let mut pool = Vec::new();
    let mut keys = Vec::new();
    for frame in 0..frame_total() {
        // Back and forth across the front, twice through the frames.
        let turn =
            (frame as f32 * (4.0 * std::f32::consts::PI / frame_total() as f32)).sin() * 110.0;
        let sway = (frame as f32 * 0.4).sin() * 15.0;
        for entry in [
            key([0.0, 0.0, 1.0], 0.0, [0.0, 0.0, 0.0]),
            key([0.15, 0.1, 1.0], turn, [0.0, 0.0, 4.0]),
            key([1.0, 0.0, 0.2], sway * 0.3, [0.0, 0.0, 3.0]),
            key([0.0, 1.0, 0.3], sway, [0.0, 0.0, 3.0]),
            key([0.0, 0.0, 1.0], turn * 0.25, [0.0, 0.0, 0.0]),
        ] {
            keys.push(pool.len() as u32);
            pool.push(entry);
        }
    }
    let vertex = |position, bone: u16| TagVertex {
        position,
        influences: vec![(bone, 1023)],
    };
    let hand = |side: f32| Tag {
        vertices: [
            vertex([12.0, 3.0 * side, 0.0], 3),
            vertex([13.0, 3.0 * side, 0.0], 3),
            vertex([12.0, 4.0 * side, 0.0], 3),
        ],
    };
    let blade = |way: f32| SaberBlade {
        tag: [[0.0, 0.0, 0.0], [way, 0.0, 0.0], [0.0, way, 0.0]],
        length: 40.0,
    };
    SkeletonData::new(SkeletonParts {
        bones,
        frame_count: frame_total(),
        keys,
        pool,
        hands: [hand(-1.0), hand(1.0)],
        sabers: vec![
            Saber {
                name: "single".into(),
                blades: vec![blade(1.0)],
            },
            Saber {
                name: "staff".into(),
                blades: vec![blade(1.0), blade(-1.0)],
            },
        ],
        motion: Some(4),
    })
    .map_err(|e| e.to_string())
}

/// Which animations a duel's table leaves out (`anims`).
#[derive(Clone, PartialEq)]
enum Missing {
    None,
    /// About one in nine.
    Sparse,
    /// Every one whose name holds this.
    Named(String),
}

/// The animations: each a run of the skeleton's frames found from its
/// name (4 to 33 frames of 30 to 79 ms), with the skeleton; some left out
/// as `missing` says.
fn duel_anims(missing: &Missing) -> Result<AnimTable, String> {
    let table = AnimTable::from_lookup(|name| {
        let h = blake3::hash(name.as_bytes());
        let b = h.as_bytes();
        let gone = match missing {
            Missing::None => false,
            Missing::Sparse => b[4].is_multiple_of(9),
            Missing::Named(text) => name.contains(text.as_str()),
        };
        if gone {
            return None;
        }
        let frames = 4 + i32::from(b[1] % 30);
        Some(AnimInfo {
            first_frame: i32::from(b[0]) % (frame_total() as i32 - frames),
            frames,
            frame_lerp: 30 + i32::from(b[2] % 50),
            feet: Some([[2.0, 6.0], [-2.0, -6.0]]),
            looping: b[3].is_multiple_of(4),
        })
    });
    Ok(table.with_skeleton(Arc::new(duel_skeleton()?)))
}

/// The arena's floor with the made-up skeleton's animations.
fn duel_level(friendly: FriendlyFire, missing: &Missing) -> Result<Level, String> {
    type Anims = std::sync::OnceLock<Result<Arc<AnimTable>, String>>;
    static FULL: Anims = std::sync::OnceLock::new();
    static SPARSE: Anims = std::sync::OnceLock::new();
    let anims = match missing {
        Missing::None => FULL
            .get_or_init(|| duel_anims(missing).map(Arc::new))
            .clone()?,
        Missing::Sparse => SPARSE
            .get_or_init(|| duel_anims(missing).map(Arc::new))
            .clone()?,
        Missing::Named(_) => Arc::new(duel_anims(missing)?),
    };
    let arena = Level::arena();
    Ok(Level::new("m6", arena.collision().clone(), anims).with_friendly_fire(friendly))
}

/// A module for the level, with nothing placed: what the Force needs to
/// price its powers (every row `cost` points).
fn duel_module(cost: u16) -> Result<crate::module::ModuleData, String> {
    let ground = crate::determinism::proving_ground();
    let data = ground.module().ok_or("the proving ground has no module")?;
    Ok(crate::module::ModuleData {
        name: "m6".into(),
        placements: Vec::new(),
        entry: crate::module::Spot {
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
        },
        force_costs: vec![cost; 256],
        // `repute.2da`'s first rows: each faction its own friend, the
        // hostile ones (1, 3) at war with the player's (0) and the friendly (2), and
        // the rest neutral.
        repute: crate::module::Repute {
            rows: (0..8u8)
                .map(|a| {
                    (0..8u8)
                        .map(|b| match (a, b) {
                            _ if a == b => 100,
                            (0, 4) | (4, 0) => 100,
                            (1 | 3, 0 | 2) | (0 | 2, 1 | 3) => 0,
                            _ => 50,
                        })
                        .collect()
                })
                .collect(),
        },
        ..(**data).clone()
    })
}

fn lightsaber(row: u16) -> Weapon {
    Weapon::plain(
        row,
        2,
        8,
        (2, 2),
        damage_types::ENERGY,
        [
            feats::PROFICIENCY_LIGHTSABER,
            feats::WEAPON_FOCUS_LIGHTSABER,
            feats::WEAPON_SPEC_LIGHTSABER,
        ],
    )
}

fn blaster(row: u16) -> Weapon {
    Weapon::plain(
        row,
        1,
        6,
        (1, 2),
        damage_types::ENERGY,
        [
            feats::PROFICIENCY_BLASTER,
            feats::WEAPON_FOCUS_BLASTER,
            feats::WEAPON_SPEC_BLASTER,
        ],
    )
}

/// A seat's fighter before the world is made.
#[derive(Clone)]
struct Setup {
    origin: [f32; 3],
    yaw: f32,
    sheet: Sheet,
}

/// A fighter of `kind` from `key=value`s.
fn setup(seat: u8, kind: &str, a: &[String]) -> Result<Setup, String> {
    let mut sheet = Sheet::commoner();
    sheet.faction = crate::combat::sheet::factions::PLAYER;
    sheet.weapons = match kind {
        "fast" | "medium" | "strong" => [Some(lightsaber(8)), None],
        "dual" => [Some(lightsaber(8)), Some(lightsaber(8))],
        "staff" => [Some(lightsaber(9)), None],
        "pistol" => [Some(blaster(12)), None],
        "ion" => [Some(blaster(15)), None],
        "rifle" => [Some(blaster(13)), None],
        "fists" => [None, None],
        other => return Err(format!("no fighter kind {other}")),
    };
    let mut origin = [60.0 * f32::from(seat), 0.0, 0.125 - motion::SOLE_Z];
    let mut yaw = if seat == 0 { 0.0 } else { 180.0 };
    let (mut defence, mut jump, mut throw, mut tiers) = (2usize, 2usize, 0u8, 3usize);
    let mut proficient = true;
    sheet.max_hp = 60;
    sheet.max_fp = 100;
    for (key, value) in pairs(a) {
        match key {
            "x" => origin[0] = float(value)?,
            "y" => origin[1] = float(value)?,
            "z" => origin[2] = float(value)?,
            "yaw" => yaw = float(value)?,
            "defence" => defence = int(value)?,
            "jump" => jump = int(value)?,
            "throw" => throw = int(value)?,
            "tiers" => tiers = int(value)?,
            "plot" => {
                if value == "1" {
                    sheet.flags |= crate::combat::sheet::flags::PLOT;
                }
            }
            "hp" => sheet.max_hp = int(value)?,
            "fp" => sheet.max_fp = int(value)?,
            // The `spells.2da` rows it knows, joined by commas.
            "powers" => {
                for row in value.split(',') {
                    sheet.powers.insert(int(row)?);
                }
            }
            "proficient" => proficient = value != "0",
            other => return Err(format!("fighter: no key {other}")),
        }
    }
    if proficient {
        sheet.feats.insert(feats::PROFICIENCY_LIGHTSABER);
        sheet.feats.insert(feats::PROFICIENCY_BLASTER);
    }
    let lines: [&[u16]; 4] = [
        &feats::CRITICAL_STRIKE,
        &feats::FLURRY,
        &feats::POWER_ATTACK,
        &feats::TWO_WEAPON_FIGHTING,
    ];
    for line in lines {
        for row in line.iter().take(tiers) {
            sheet.feats.insert(*row);
        }
    }
    for row in feats::JEDI_DEFENSE.iter().take(defence) {
        sheet.feats.insert(*row);
    }
    for row in feats::FORCE_JUMP.iter().take(jump) {
        sheet.feats.insert(*row);
    }
    match throw {
        0 => {}
        1 => sheet.powers.insert(49),
        _ => sheet.powers.insert(4),
    }
    Ok(Setup { origin, yaw, sheet })
}

/// A seat's keys for a run of ticks.
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    forward: i8,
    right: i8,
    up: i8,
    buttons: u16,
    turn: f32,
    weapon: u8,
    /// The power the force button casts (`Command::force`).
    power: u8,
    /// Degrees looked up (negative) or down from where it would look.
    pitch: f32,
    /// A creature to look at (its number among the placed) instead of the
    /// next seat.
    at: Option<u32>,
}

fn button_names(word: &str) -> Result<u16, String> {
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
            "style" => buttons::SABER_STYLE,
            "force" => buttons::FORCE_POWER,
            "grip" => buttons::FORCE_GRIP,
            "lightning" => buttons::FORCE_LIGHTNING,
            "drain" => buttons::FORCE_DRAIN,
            other => return Err(format!("no button {other}")),
        };
    }
    Ok(bits)
}

/// The duel: its world, level and last snapshot.
struct Duel {
    seed: u64,
    friendly: FriendlyFire,
    missing: Missing,
    /// The Force's price of every power, with a module (`None`: none).
    costs: Option<u16>,
    /// Why each tick steps (`reason predict|authority`).
    reason: StepReason,
    /// Creatures placed in the module: each its fighter and faction.
    creatures: Vec<(Setup, u8)>,
    setups: Vec<Setup>,
    world: Option<(SimWorld, Level, Snapshot)>,
    lags: Vec<u16>,
    seen: std::collections::BTreeSet<(u32, u16)>,
}

impl Duel {
    fn new() -> Duel {
        Duel {
            seed: 1,
            friendly: FriendlyFire::Full,
            missing: Missing::None,
            costs: None,
            creatures: Vec::new(),
            reason: StepReason::Authority,
            setups: Vec::new(),
            world: None,
            lags: Vec::new(),
            seen: std::collections::BTreeSet::new(),
        }
    }

    fn begin(&mut self) -> Result<(), String> {
        if self.setups.is_empty() {
            return Err("begin: no fighters".to_string());
        }
        let mut level = duel_level(self.friendly, &self.missing)?;
        if !self.creatures.is_empty() && self.costs.is_none() {
            self.costs = Some(10);
        }
        if let Some(cost) = self.costs {
            let mut module = duel_module(cost)?;
            for (setup, faction) in &self.creatures {
                let mut sheet = setup.sheet;
                sheet.faction = *faction;
                let at = setup.origin.map(|v| v * collision::UNIT);
                module
                    .placements
                    .push(crate::module::Placement::Own(crate::module::ObjectData {
                        sheet: Some(sheet),
                        body: Some(crate::module::Body {
                            appearance: 0,
                            size: 3,
                            movement: 4,
                            walk_mps: 1.7,
                            run_mps: 5.4,
                            head_arc_h: 0.0,
                            bag: 0,
                            destroy_ms: 0,
                            fade_ms: None,
                        }),
                        yaw: setup.yaw,
                        ..crate::module::ObjectData::bare(
                            crate::module::ObjectKind::Creature,
                            "fencer",
                            [at[0], at[1], 0.0],
                        )
                    }));
            }
            level = level.with_module(module);
        }
        let config = WorldConfig {
            seed: self.seed,
            seats: self.setups.len() as u8,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level.clone()).map_err(|e| e.to_string())?;
        let mut start = world.snapshot();
        for (seat, setup) in self.setups.iter().enumerate() {
            let entity = start
                .entities
                .iter_mut()
                .find(|e| e.kind == Kind::Avatar(Seat(seat as u8)))
                .ok_or("begin: a seat missing")?;
            entity.player = Some(MotionState::standing(setup.origin, setup.yaw));
            entity.sheet = Some(setup.sheet);
            entity.fighter = Some(Fighter::of(&setup.sheet));
        }
        let world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        self.lags = vec![0; self.setups.len()];
        self.world = Some((world, level, start));
        Ok(())
    }

    fn snapshot(&self) -> Result<&Snapshot, String> {
        self.world
            .as_ref()
            .map(|(_, _, last)| last)
            .ok_or_else(|| "before begin".to_string())
    }

    fn ps(&self, seat: u8) -> Result<MotionState, String> {
        self.snapshot()?
            .own(Seat(seat))
            .and_then(|e| e.player)
            .ok_or_else(|| format!("no player in seat {seat}"))
    }

    /// The world made again from its snapshot changed by `change`.
    fn change(
        &mut self,
        seat: u8,
        change: impl FnOnce(&mut crate::EntityState) -> Result<(), String>,
    ) -> Result<(), String> {
        let (_, level, last) = self.world.as_mut().ok_or("before begin")?;
        let mut start = last.clone();
        let entity = start
            .entities
            .iter_mut()
            .find(|e| e.kind == Kind::Avatar(Seat(seat)))
            .ok_or_else(|| format!("no seat {seat}"))?;
        change(entity)?;
        let world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        let level = level.clone();
        self.world = Some((world, level, start));
        Ok(())
    }

    /// `change` on a seat (`"0"`) or a creature (`"c0"`).
    fn change_who(
        &mut self,
        who: &str,
        change: impl FnOnce(&mut crate::EntityState) -> Result<(), String>,
    ) -> Result<(), String> {
        let (_, level, last) = self.world.as_mut().ok_or("before begin")?;
        let mut start = last.clone();
        let wanted = match who.strip_prefix('c') {
            Some(n) => Kind::Object(crate::module::ObjectKind::Creature, int(n)?),
            None => Kind::Avatar(Seat(int(who)?)),
        };
        let entity = start
            .entities
            .iter_mut()
            .find(|e| e.kind == wanted)
            .ok_or_else(|| format!("no {who}"))?;
        change(entity)?;
        let world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        let level = level.clone();
        self.world = Some((world, level, start));
        Ok(())
    }

    /// A lock of `kind` started between seats `att` and `def` in the
    /// snapshot, as a clash leaves one (`lock::opening`'s animations and
    /// frames, the attack held as `pressed`, both facing, the lock's
    /// distance apart along the line); the world made again from it.
    fn lock(&mut self, att: u8, def: u8, kind: Kind2) -> Result<(), String> {
        use crate::combat::saber::lock;
        let (_, level, last) = self.world.as_mut().ok_or("before begin")?;
        let mut start = last.clone();
        let anims = level.anims().clone();
        let find = |s: &Snapshot, seat: u8| {
            s.entities
                .iter()
                .position(|e| e.kind == Kind::Avatar(Seat(seat)))
                .ok_or_else(|| format!("no seat {seat}"))
        };
        let (ia, id) = (find(&start, att)?, find(&start, def)?);
        let style = |e: &crate::EntityState| {
            e.fighter
                .and_then(|f| f.saber.style)
                .unwrap_or(Style::Medium)
        };
        let opening = lock::opening(
            kind.0,
            style(&start.entities[ia]),
            style(&start.entities[id]),
        );
        let now = start.time_ms;
        let ids = (start.entities[ia].id, start.entities[id].id);
        let at = start.entities[ia].player.ok_or("no player")?.origin;
        for (index, (anim, fraction), enemy, yaw) in [
            (ia, opening.attacker, ids.1, 0.0f32),
            (id, opening.defender, ids.0, 180.0),
        ] {
            let entity = &mut start.entities[index];
            let mut ps = entity.player.ok_or("no player")?;
            ps.origin = if index == id {
                [at[0] + opening.distance, at[1], at[2]]
            } else {
                at
            };
            ps.set_view([0.0, yaw, 0.0]);
            crate::motion::react(&mut ps, &anims, anim, false);
            ps.velocity = [0.0; 3];
            entity.player = Some(ps);
            let f = entity.fighter.as_mut().ok_or("no fighter")?;
            f.saber.lock = Some(crate::combat::saber::Lock {
                enemy,
                anim,
                frame: lock::first_frame(&anims, anim, fraction),
                hits: 0,
                advance: false,
                pressed: f.last_command.buttons & buttons::ATTACK != 0,
                checked_ms: now,
            });
            f.saber.lock_until = now + lock::LOCK_MS;
            f.saber.blocked = crate::combat::saber::Blocked::None;
            f.saber.current_move = Move::NONE;
            f.weapon_wait_ms = 0;
        }
        let world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        let level = level.clone();
        self.world = Some((world, level, start));
        Ok(())
    }

    /// One tick of every seat's keys; recorded.
    fn tick(&mut self, keys: &[Keys], step: u64, trace: &mut Trace) -> Result<(), String> {
        let seats = self.setups.len() as u8;
        let mut input = TickInput::default();
        for seat in 0..seats {
            let k = keys
                .get(usize::from(seat))
                .copied()
                .unwrap_or(Keys::default());
            let me = self.ps(seat)?;
            let them = self.ps((seat + 1) % seats.max(1))?;
            let gun = self.setups[usize::from(seat)]
                .sheet
                .main_weapon()
                .delivery
                .ranged();
            let from = if gun {
                mind::crosshair(&me)
            } else {
                mind::eye(&me)
            };
            let creature = k.at.and_then(|n| {
                self.snapshot().ok()?.entities.iter().find_map(|e| {
                    (e.kind == Kind::Object(crate::module::ObjectKind::Creature, n))
                        .then_some(e.player)
                        .flatten()
                })
            });
            let (pitch, yaw) = match creature {
                Some(target) => mind::aim(from, mind::chest(&target)),
                None if seats > 1 => mind::aim(from, mind::chest(&them)),
                None => (0.0, me.view[1]),
            };
            let pitch = pitch + k.pitch;
            let command = Command {
                forward: k.forward,
                right: k.right,
                up: k.up,
                buttons: k.buttons,
                weapon: k.weapon,
                force: k.power,
                ..mind::looking(&me, pitch, yaw + k.turn)
            };
            input.insert(Seat(seat), command);
            input.set_view_lag(Seat(seat), self.lags[usize::from(seat)]);
        }
        let (world, _, last) = self.world.as_mut().ok_or("before begin")?;
        let before = last.authority;
        let tick = last.tick + 1;
        let msec = TickRate::default().msec(tick);
        *last = crate::step(world, tick, &input, msec, self.reason);
        let mut r = Record::new(step, "tick");
        r.i("tick", last.tick).i("t", last.time_ms);
        // The seats, then the creatures by their number among the placed.
        let mut who: Vec<(String, &crate::EntityState)> = (0..seats)
            .filter_map(|seat| last.own(Seat(seat)).map(|e| (seat.to_string(), e)))
            .collect();
        who.extend(last.entities.iter().filter_map(|e| match e.kind {
            Kind::Object(crate::module::ObjectKind::Creature, n) => Some((format!("c{n}"), e)),
            _ => None,
        }));
        for (label, entity) in who {
            let k = |name: &str| format!("{label}.{name}");
            if let Some(ps) = entity.player {
                r.v3(&k("o"), ps.origin)
                    .v3(&k("v"), ps.velocity)
                    .v3(&k("view"), ps.view)
                    .s(&k("legs"), ps.legs.anim.name())
                    .i(&k("lt"), ps.legs.timer)
                    .s(&k("torso"), ps.torso.anim.name())
                    .i(&k("tt"), ps.torso.timer)
                    .i(&k("fl"), ps.flags);
            }
            if let Some(f) = entity.fighter {
                let s = &f.saber;
                r.i(&k("hp"), f.hp)
                    .i(&k("fp"), f.fp)
                    .i(&k("wms"), f.weapon_wait_ms)
                    .b(&k("on"), s.on)
                    .s(&k("sty"), s.style.map_or("-", Style::name))
                    .s(&k("mv"), s.current_move.name())
                    .d(&k("bkd"), &s.blocked)
                    .d(&k("blk"), &s.blocking)
                    .i(&k("chn"), s.chain)
                    .d(&k("hnd"), &s.hand)
                    .i(&k("hu"), s.hand_until)
                    .d(&k("gu"), &s.getup)
                    .i(&k("lku"), s.lock_until)
                    .i(&k("bbu"), s.bolt_block_until);
                r.d(&k("swing"), &f.swing).d(&k("force"), &f.force);
                let mut encoded = crate::wire::Writer::default();
                f.encode(&mut encoded);
                r.hex(&k("fenc"), &blake3::hash(&encoded.0).as_bytes()[..8]);
                for (i, trail) in f.trails.iter().enumerate() {
                    if let Some(trail) = trail {
                        r.v3(&k(&format!("bm{i}")), trail.muzzle)
                            .v3(&k(&format!("bd{i}")), trail.direction)
                            .i(&k(&format!("bt{i}")), trail.at_ms);
                    }
                }
                match s.lock {
                    None => r.s(&k("lock"), "-"),
                    Some(lock) => r.d(&k("lock"), &lock),
                };
                match s.thrown {
                    None => r.s(&k("thr"), "-"),
                    Some(t) => r
                        .d(&k("thr"), &t.flight)
                        .v3(&k("thr.at"), t.at)
                        .v3(&k("thr.v"), t.velocity)
                        .i(&k("thr.since"), t.since)
                        .i(&k("thr.hit"), t.hit_until),
                };
                let mut new: Vec<_> = f
                    .events
                    .iter()
                    .flatten()
                    .filter(|e| self.seen.insert((entity.id.0, e.sequence)))
                    .copied()
                    .collect();
                new.sort_by_key(|e| e.sequence);
                if !new.is_empty() {
                    r.d(&k("ev"), &new);
                }
            }
        }
        // The server's stream, and how many numbers the tick drew from it.
        let mut state = crate::wire::Writer::default();
        last.authority.encode(&mut state);
        r.hex("rng", &state.0);
        let mut drawn = before;
        let draws = (0..=4096)
            .find(|_| {
                let same = drawn == last.authority;
                drawn.next_u32();
                same
            })
            .map_or(">4096".to_string(), |n| n.to_string());
        r.s("draws", &draws);
        r.hex("hash", &last.hash().0[..12]);
        trace.put(&r)
    }
}

/// `tick` keys by seat: `f0 r0 u0 b0 t0 w0`, `f1 ...`.
fn tick_keys(a: &[String], seats: usize) -> Result<(u64, Vec<Keys>), String> {
    let mut n = 1;
    let mut keys = vec![Keys::default(); seats];
    for word in a {
        let Some((key, value)) = word.split_once('=') else {
            n = int(word)?;
            continue;
        };
        let (field, seat) = key.split_at(key.len().saturating_sub(1));
        let seat: usize = int(seat)?;
        let k = keys
            .get_mut(seat)
            .ok_or_else(|| format!("tick: no seat {seat}"))?;
        match field {
            "f" => k.forward = int(value)?,
            "r" => k.right = int(value)?,
            "u" => k.up = int(value)?,
            "b" => k.buttons = button_names(value)?,
            "t" => k.turn = float(value)?,
            "l" => k.pitch = float(value)?,
            "c" => k.at = Some(int(value)?),
            "w" => k.weapon = int(value)?,
            "p" => k.power = int(value)?,
            other => return Err(format!("tick: no key {other}")),
        }
    }
    Ok((n, keys))
}

/// Runs a duel; the ticks it recorded.
pub fn run(scenario: &Scenario, trace: &mut Trace) -> Result<u64, String> {
    let mut duel = Duel::new();
    let mut step = 0;
    for line in &scenario.lines {
        let a = line.args();
        let word = |i: usize| -> Result<&str, String> {
            a.get(i)
                .map(String::as_str)
                .ok_or_else(|| format!("too few words: {}", line.place()))
        };
        let at = |e: String| format!("{e} ({})", line.place());
        match line.verb() {
            "seed" => duel.seed = int(word(0)?)?,
            "module" => {
                duel.costs = match word(0)? {
                    "none" => None,
                    cost => Some(int(cost)?),
                }
            }
            "anims" => {
                duel.missing = match word(0)? {
                    "full" => Missing::None,
                    "sparse" => Missing::Sparse,
                    "without" => Missing::Named(word(1)?.to_string()),
                    other => return Err(format!("no anims {other} (full, sparse, without)")),
                }
            }
            "friendly" => {
                duel.friendly = match word(0)? {
                    "off" => FriendlyFire::Off,
                    "area" => FriendlyFire::Area,
                    "full" => FriendlyFire::Full,
                    other => return Err(format!("no friendly fire {other}")),
                }
            }
            "fighter" => {
                let seat: u8 = int(word(0)?)?;
                if usize::from(seat) != duel.setups.len() {
                    return Err(at(format!("fighter {seat}: seats in order from 0")));
                }
                duel.setups
                    .push(setup(seat, word(1)?, a.get(2..).unwrap_or(&[])).map_err(at)?);
            }
            // `creature KIND [faction=hostile|friendly|ally] [fighter keys]`: one
            // placed in the module, fighting by its own mind.
            "creature" => {
                let mut faction = crate::combat::sheet::factions::HOSTILE_1;
                let rest: Vec<String> = a
                    .get(1..)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|w| match w.strip_prefix("faction=") {
                        Some("friendly") => {
                            faction = crate::combat::sheet::factions::FRIENDLY_1;
                            false
                        }
                        // A friend of the players (`repute` 100 both ways).
                        Some("ally") => {
                            faction = crate::combat::sheet::factions::FRIENDLY_2;
                            false
                        }
                        Some(_) => false,
                        None => true,
                    })
                    .cloned()
                    .collect();
                let index = duel.creatures.len() as u8;
                let made = setup(index + 1, word(0)?, &rest).map_err(at)?;
                duel.creatures.push((made, faction));
            }
            "begin" => duel.begin().map_err(at)?,
            "tick" => {
                let (n, keys) = tick_keys(a, duel.setups.len()).map_err(at)?;
                for _ in 0..n {
                    duel.tick(&keys, step, trace).map_err(at)?;
                    step += 1;
                }
            }
            // `until SEAT locked|unlocked|down N [keys]`: up to N ticks,
            // ending on the one after which the seat is so.
            "until" => {
                let (seat, state): (u8, &str) = (int(word(0)?)?, word(1)?);
                let (n, keys) =
                    tick_keys(a.get(2..).unwrap_or(&[]), duel.setups.len()).map_err(at)?;
                for _ in 0..n {
                    duel.tick(&keys, step, trace).map_err(at)?;
                    step += 1;
                    let fighter = duel
                        .snapshot()
                        .map_err(at)?
                        .own(Seat(seat))
                        .and_then(|e| e.fighter)
                        .ok_or_else(|| at(format!("no fighter in seat {seat}")))?;
                    let done = match state {
                        "locked" => fighter.saber.lock.is_some(),
                        "unlocked" => fighter.saber.lock.is_none(),
                        "down" => fighter.saber.floored(),
                        "gripped" => fighter.force.gripped_by.is_some(),
                        other => return Err(at(format!("until: no state {other}"))),
                    };
                    if done {
                        break;
                    }
                }
            }
            "lag" => {
                let (seat, ms): (usize, u16) = (int(word(0)?)?, int(word(1)?)?);
                *duel.lags.get_mut(seat).ok_or("lag: no seat")? = ms;
            }
            // `lock ATT DEF KIND`: a lock of `KIND` (`top diag_tr diag_tl
            // diag_br diag_bl r l`) started between the two seats.
            "lock" => {
                use crate::combat::saber::lock::Kind as K;
                let (att, def): (u8, u8) = (int(word(0)?)?, int(word(1)?)?);
                let kind = match word(2)? {
                    "top" => K::Top,
                    "diag_tr" => K::TopRight,
                    "diag_tl" => K::TopLeft,
                    "diag_br" => K::BottomRight,
                    "diag_bl" => K::BottomLeft,
                    "r" => K::R,
                    "l" => K::L,
                    other => return Err(at(format!("no lock kind {other}"))),
                };
                duel.lock(att, def, Kind2(kind)).map_err(at)?;
            }
            // `put WHO X Y Z YAW`: a seat or a creature stood there.
            "reason" => {
                duel.reason = match word(0)? {
                    "authority" => StepReason::Authority,
                    "predict" => StepReason::Predict,
                    other => return Err(at(format!("no reason {other}"))),
                }
            }
            // `stat WHO hp|fp N` (hundredths).
            "stat" => {
                let (who, what) = (word(0)?.to_string(), word(1)?.to_string());
                let n: i32 = int(word(2)?)?;
                duel.change_who(&who, |e| {
                    let f = e.fighter.as_mut().ok_or("no fighter")?;
                    match what.as_str() {
                        "hp" => f.hp = n,
                        "fp" => f.fp = n,
                        other => return Err(format!("stat: no {other}")),
                    }
                    Ok(())
                })
                .map_err(at)?;
            }
            // `force WHO key=value`: its Force state's timers (ms from now):
            // `hand throw_ready grip_ready drain_ready stunned immune`.
            "force" => {
                let who = word(0)?.to_string();
                let changes: Vec<(String, u64)> = pairs(a.get(1..).unwrap_or(&[]))
                    .into_iter()
                    .map(|(k, v)| int(v).map(|v| (k.to_string(), v)))
                    .collect::<Result<_, _>>()
                    .map_err(at)?;
                let now = duel.snapshot().map_err(at)?.time_ms;
                duel.change_who(&who, |e| {
                    let force = &mut e.fighter.as_mut().ok_or("no fighter")?.force;
                    for (key, ms) in &changes {
                        let until = now + ms;
                        match key.as_str() {
                            "hand" => {
                                force.hand = Some((crate::combat::force::Gesture::Push, until));
                            }
                            "throw_ready" => force.push_pull_ready = until,
                            "grip_ready" => force.grip_ready = until,
                            "drain_ready" => force.drain_ready = until,
                            "stunned" => force.stunned_until = until,
                            "immune" => force.immune_until = until,
                            other => return Err(format!("force: no key {other}")),
                        }
                    }
                    Ok(())
                })
                .map_err(at)?;
            }
            // `knockback WHO MS`: thrown back (its knockback flag) for MS.
            "knockback" => {
                let who = word(0)?.to_string();
                let ms: i32 = int(word(1)?)?;
                duel.change_who(&who, |e| {
                    let ps = e.player.as_mut().ok_or("no player")?;
                    ps.flags |= crate::motion::motion_flags::KNOCKBACK_HOLD;
                    ps.hold_ms = ms;
                    Ok(())
                })
                .map_err(at)?;
            }
            // `pose WHO ANIM MS`: its torso playing ANIM, held MS.
            "pose" => {
                let who = word(0)?.to_string();
                let anim = crate::motion::Anim::from_name(word(1)?)
                    .ok_or_else(|| at(format!("no animation {}", a[1])))?;
                let ms: i32 = int(word(2)?)?;
                duel.change_who(&who, |e| {
                    let ps = e.player.as_mut().ok_or("no player")?;
                    ps.torso.anim = anim;
                    ps.torso.timer = ms;
                    Ok(())
                })
                .map_err(at)?;
            }
            // `away WHO FLIGHT LEVEL`: its saber away from the hand, thrown
            // at LEVEL (a creature's too).
            "away" => {
                let who = word(0)?.to_string();
                let flight = match word(1)? {
                    "leaving" => Flight::Leaving,
                    "out" => Flight::Out,
                    "back" => Flight::Back,
                    "falling" => Flight::Falling,
                    "lying" => Flight::Lying,
                    other => return Err(at(format!("no flight {other}"))),
                };
                let level: u8 = int(word(2)?)?;
                let now = duel.snapshot().map_err(at)?.time_ms;
                duel.change_who(&who, |e| {
                    let f = e.fighter.as_mut().ok_or("no fighter")?;
                    let hand = e.player.map_or([0.0; 3], |ps| ps.origin);
                    f.saber.thrown = Some(Thrown {
                        flight,
                        at: [hand[0], hand[1], hand[2] + 16.0],
                        ..Thrown::leaving(level, now)
                    });
                    Ok(())
                })
                .map_err(at)?;
            }
            "put" => {
                let who = word(0)?.to_string();
                let (x, y, z, yaw) = (
                    float(word(1)?)?,
                    float(word(2)?)?,
                    float(word(3)?)?,
                    float(word(4)?)?,
                );
                duel.change_who(&who, |e| {
                    e.player = Some(MotionState::standing([x, y, z], yaw));
                    Ok(())
                })
                .map_err(at)?;
            }
            // `roundtrip`: the world's snapshot written to bytes, read back,
            // and the world made again from what was read.
            "roundtrip" => {
                let (_, level, last) = duel
                    .world
                    .as_mut()
                    .ok_or_else(|| at("before begin".into()))?;
                let read = Snapshot::decode(&last.encode()).map_err(|e| at(e.to_string()))?;
                let world =
                    SimWorld::from_snapshot(&read, level.clone()).map_err(|e| at(e.to_string()))?;
                let level = level.clone();
                duel.world = Some((world, level, read));
            }
            // `shift WHO DX DY DZ`: moved by so much, all else kept.
            "shift" => {
                let who = word(0)?.to_string();
                let d = [float(word(1)?)?, float(word(2)?)?, float(word(3)?)?];
                duel.change_who(&who, |e| {
                    let ps = e.player.as_mut().ok_or("no player")?;
                    ps.origin = [
                        ps.origin[0] + d[0],
                        ps.origin[1] + d[1],
                        ps.origin[2] + d[2],
                    ];
                    Ok(())
                })
                .map_err(at)?;
            }
            // `toggle WHO absorb|protect LEVEL MS`: a seat (`0`) or a
            // creature (`c0`) absorbing or protected at LEVEL for MS.
            "toggle" => {
                let (who, what) = (word(0)?.to_string(), word(1)?.to_string());
                let (level, ms): (u8, u64) = (int(word(2)?)?, int(word(3)?)?);
                let now = duel.snapshot().map_err(at)?.time_ms;
                duel.change_who(&who, |e| {
                    let force = &mut e.fighter.as_mut().ok_or("no fighter")?.force;
                    match what.as_str() {
                        "absorb" => {
                            force.absorb_level = level;
                            force.absorb_until = now + ms;
                        }
                        "protect" => {
                            force.protect_level = level;
                            force.protect_until = now + ms;
                        }
                        other => return Err(format!("toggle: no {other}")),
                    }
                    Ok(())
                })
                .map_err(at)?;
            }
            "wound" => {
                let (seat, hp): (u8, i32) = (int(word(0)?)?, int(word(1)?)?);
                duel.change(seat, |e| {
                    let f = e.fighter.as_mut().ok_or("no fighter")?;
                    f.hp = hp;
                    Ok(())
                })
                .map_err(at)?;
            }
            "place" => {
                let seat: u8 = int(word(0)?)?;
                let (x, y, z, yaw) = (
                    float(word(1)?)?,
                    float(word(2)?)?,
                    float(word(3)?)?,
                    float(word(4)?)?,
                );
                duel.change(seat, |e| {
                    e.player = Some(MotionState::standing([x, y, z], yaw));
                    Ok(())
                })
                .map_err(at)?;
            }
            "saber" => {
                let seat: u8 = int(word(0)?)?;
                let changes: Vec<(String, String)> = pairs(a.get(1..).unwrap_or(&[]))
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                let now = duel.snapshot().map_err(at)?.time_ms;
                duel.change(seat, |e| {
                    let s = &mut e.fighter.as_mut().ok_or("no fighter")?.saber;
                    for (key, value) in &changes {
                        match key.as_str() {
                            "on" => s.on = value == "1",
                            "style" => {
                                s.style = match value.as_str() {
                                    "fast" => Some(Style::Fast),
                                    "medium" => Some(Style::Medium),
                                    "strong" => Some(Style::Strong),
                                    "dual" => Some(Style::Dual),
                                    "staff" => Some(Style::Staff),
                                    other => return Err(format!("no style {other}")),
                                }
                            }
                            "move" => {
                                s.current_move = Move::ALL
                                    .iter()
                                    .copied()
                                    .find(|m| m.name() == value.as_str())
                                    .ok_or_else(|| format!("no move {value}"))?;
                            }
                            "lock_until" => s.lock_until = now + int::<u64>(value)?,
                            "lock" if value == "none" => s.lock = None,
                            "thrown" => {
                                s.thrown = match value.as_str() {
                                    "none" => None,
                                    "falling" => Some(Flight::Falling),
                                    "lying" => Some(Flight::Lying),
                                    "out" => Some(Flight::Out),
                                    "back" => Some(Flight::Back),
                                    other => return Err(format!("no flight {other}")),
                                }
                                .map(|flight| Thrown {
                                    flight,
                                    ..Thrown::leaving(1, now)
                                });
                            }
                            other => return Err(format!("saber: no key {other}")),
                        }
                    }
                    Ok(())
                })
                .map_err(at)?;
            }
            other => return Err(at(format!("no directive {other}"))),
        }
    }
    Ok(step)
}

/// The kinds a random duel's fighters are: mostly blades.
const KINDS: &[&str] = &[
    "fast", "medium", "strong", "dual", "staff", "fast", "medium", "strong", "dual", "staff",
    "pistol", "rifle",
];

/// `n` ticks of duels for `seed`, in duels of 200 to 900 ticks: two
/// fighters (now and then three) at random kinds, tiers and spacing,
/// with spells of keys a fencer would press, a wound or a lag now and
/// then.
pub fn random(seed: u64, n: u64) -> Vec<Scenario> {
    let mut g = Gen::new(seed, 0x6d36);
    let mut out = Vec::new();
    let mut left = n;
    let mut part = 0;
    while left > 0 {
        let ticks = left.min(u64::from(g.int(200, 900) as u32));
        let mut s = Scenario::new(format!("random-{seed}-{part}"));
        s.push_owned(vec!["seed".into(), g.u32().to_string()]);
        if g.chance(0.05) {
            s.push(&["friendly", g.pick(&["off", "area"])]);
        }
        let seats = if g.chance(0.1) { 3 } else { 2 };
        let apart = g.range(25.0, 90.0);
        for seat in 0..seats {
            let kind = if seat == 0 && g.chance(0.8) {
                g.pick(&KINDS[..10])
            } else {
                g.pick(KINDS)
            };
            let (x, y) = match seat {
                0 => (0.0, 0.0),
                1 => (apart, g.range(-10.0, 10.0)),
                _ => (apart * 0.5, g.range(40.0, 90.0)),
            };
            let yaw = if seat == 0 { 0.0 } else { 180.0 };
            s.push_owned(vec![
                "fighter".into(),
                seat.to_string(),
                kind.into(),
                format!("x={x}"),
                format!("y={y}"),
                format!("yaw={yaw}"),
                format!("defence={}", g.int(0, 3)),
                format!("jump={}", g.int(0, 3)),
                format!("throw={}", g.int(0, 2)),
                format!("tiers={}", g.int(0, 3)),
                format!("hp={}", g.pick(&[20, 60, 200])),
                format!("fp={}", g.pick(&[0, 30, 100, 300])),
                // Powers now and then: a push, a grip, a drain, protect.
                format!(
                    "powers={}",
                    g.pick(&[
                        "49",
                        "23,9,153,18",
                        "27,50,15,24",
                        "26,32,11,17",
                        "162,177,154"
                    ])
                ),
            ]);
        }
        // Now and then a creature or two, fighting by their own minds.
        if g.chance(0.3) {
            for k in 0..g.int(1, 2) {
                let kind = g.pick(&KINDS[..10]);
                let x = g.range(30.0, 120.0);
                let y = g.range(-60.0, 60.0) * k as f32;
                let mut line = vec![
                    "creature".to_string(),
                    kind.to_string(),
                    format!("x={x}"),
                    format!("y={y}"),
                    "yaw=180".to_string(),
                    format!("defence={}", g.int(0, 3)),
                    format!("tiers={}", g.int(0, 3)),
                    format!("fp={}", g.pick(&[0, 100, 300])),
                    format!("powers={}", g.pick(&["23", "9", "26,24"])),
                ];
                if g.chance(0.2) {
                    line.push("faction=friendly".into());
                }
                s.push_owned(line);
            }
        }
        s.push(&["begin"]);
        // Lit first: a press of attack, then a settle.
        let mut light = vec!["tick".to_string(), "1".to_string()];
        let mut settle = vec!["tick".to_string(), g.int(10, 150).to_string()];
        for seat in 0..seats {
            light.push(format!("b{seat}=attack"));
            settle.push(format!("t{seat}=0"));
        }
        s.push_owned(light);
        s.push_owned(settle);
        let mut left_ticks = ticks;
        while left_ticks > 0 {
            if g.chance(0.04) {
                let seat = g.below(seats);
                match g.below(4) {
                    0 => s.push_owned(vec![
                        "wound".into(),
                        seat.to_string(),
                        g.int(1, 3000).to_string(),
                    ]),
                    1 => s.push_owned(vec![
                        "lag".into(),
                        seat.to_string(),
                        g.pick(&[0, 50, 150, 400]).to_string(),
                    ]),
                    2 => s.push_owned(vec![
                        "saber".into(),
                        seat.to_string(),
                        format!(
                            "thrown={}",
                            g.pick(&["none", "falling", "lying", "out", "back"])
                        ),
                    ]),
                    _ => {
                        let angle = g.range(0.0, 360.0).to_radians();
                        let reach = g.range(25.0, 120.0);
                        s.push_owned(vec![
                            "place".into(),
                            seat.to_string(),
                            (reach * angle.cos()).to_string(),
                            (reach * angle.sin()).to_string(),
                            (0.125 - motion::SOLE_Z).to_string(),
                            g.range(-180.0, 180.0).to_string(),
                        ]);
                    }
                }
            }
            if g.chance(0.15) {
                // Presses of attack, as a lock's contest wants: a tick down,
                // two up; the others holding theirs or not.
                let seat = g.below(seats);
                let held: Vec<String> = (0..seats)
                    .filter(|&s| s != seat && g.chance(0.4))
                    .map(|s| format!("b{s}=attack"))
                    .collect();
                for _ in 0..g.int(3, 30) {
                    let mut down = vec!["tick".to_string(), "1".to_string()];
                    down.push(format!("b{seat}=attack"));
                    down.extend(held.iter().cloned());
                    let mut up = vec!["tick".to_string(), "2".to_string()];
                    up.extend(held.iter().cloned());
                    s.push_owned(down);
                    s.push_owned(up);
                    left_ticks = left_ticks.saturating_sub(3);
                }
                if left_ticks == 0 {
                    break;
                }
                continue;
            }
            let spell = left_ticks.min(u64::from(g.int(1, 60) as u32));
            let mut words = vec!["tick".to_string(), spell.to_string()];
            for seat in 0..seats {
                let axis = |g: &mut Gen| g.pick(&[0i8, 0, 0, 127, -127]);
                let (f, r) = (axis(&mut g), axis(&mut g));
                let u = g.pick(&[0i8, 0, 0, 0, 0, 0, 127, -127]);
                let mut bits = Vec::new();
                if g.chance(0.55) {
                    bits.push("attack");
                }
                if g.chance(0.15) {
                    bits.push("alt");
                }
                if g.chance(0.04) {
                    bits.push("style");
                }
                let mut power = None;
                match g.below(30) {
                    0 => bits.push("grip"),
                    1 => bits.push("drain"),
                    2 => {
                        bits.push("force");
                        power = Some(g.pick(&[23, 27, 26, 162, 255, 18, 24, 17]));
                    }
                    _ => {}
                }
                if let Some(power) = power {
                    words.push(format!("p{seat}={power}"));
                }
                if f != 0 {
                    words.push(format!("f{seat}={f}"));
                }
                if r != 0 {
                    words.push(format!("r{seat}={r}"));
                }
                if u != 0 {
                    words.push(format!("u{seat}={u}"));
                }
                if !bits.is_empty() {
                    words.push(format!("b{seat}={}", bits.join("+")));
                }
                if g.chance(0.2) {
                    let near = g.range(-30.0, 30.0);
                    let turn = g.pick(&[near, near, 90.0, -90.0, 180.0]);
                    words.push(format!("t{seat}={turn}"));
                }
            }
            s.push_owned(words);
            left_ticks -= spell;
        }
        out.push(s);
        left -= ticks;
        part += 1;
    }
    out
}

/// A lock's kind, as the `lock` directive names it.
#[derive(Clone, Copy)]
struct Kind2(crate::combat::saber::lock::Kind);
