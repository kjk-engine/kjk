//! A Jedi Academy level's doors and lifts: parts of the level that move
//! (`fmt_ja::bsp::Model`, each named by an entity's `model "*N"`).
//!
//! **A door** (`func_door`) slides along a direction (its `angle`, or
//! `angles`; an angle of -1 is up and -2 is down) by its size along that
//! direction less its `lip` (8 units unless the entity says). It moves at
//! its `speed` (400 units a second unless the entity says). Doors with the
//! same `team` move together, as the two halves of a double door do, and
//! are one door here ([`Mover`]). A door that starts open (`spawnflags` 1)
//! is drawn where it is closed and opens at once.
//!
//! In kjk a door opens as a KOTOR door does: a character uses it (E), and
//! it stays open. Jedi Academy's doors open by themselves as a character
//! comes near and close again after a while. kjk's rules for doors have no
//! such door.
//!
//! **A lift** (`func_plat`, or a door that moves up or down and is wide
//! and flat enough to stand on, [`is_platform`]) carries what stands on it.
//! kjk's movement has no ground that moves, so a lift stays where it rests,
//! solid.

use fmt_ja::bsp::{Bsp, Entity};

/// A door's `lip` and `speed` where its entity gives none, in units and
/// units a second.
pub const LIP: f32 = 8.0;
pub const SPEED: f32 = 400.0;

/// The entity flag of a door that starts open.
const START_OPEN: i64 = 1;

/// How wide (units) a part that moves up or down must be, along both X
/// and Y, to count as a platform to stand on; and how much wider than it
/// is tall.
const PLATFORM_WIDE: f32 = 64.0;
const PLATFORM_FLAT: f32 = 2.0;

/// What a moving part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Door,
    Lift,
}

/// A door or a lift: the level's moving parts that move as one.
#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub kind: Kind,
    /// The name scripts and triggers know it by (its `targetname`); empty
    /// for none.
    pub name: String,
    /// Its parts: the level's models (`Bsp::models`), each with how far it
    /// moves when the door opens (units).
    pub parts: Vec<(usize, [f32; 3])>,
    /// How long opening takes, in seconds.
    pub seconds: f32,
    pub starts_open: bool,
    /// The middle of its parts where they are closed (units).
    pub middle: [f32; 3],
    /// The box round its parts where they are closed (units).
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Mover {
    /// Where a door stands, as a KOTOR door does: the middle of its foot
    /// (units).
    pub fn foot(&self) -> [f32; 3] {
        [self.middle[0], self.middle[1], self.min[2]]
    }

    /// Where a character stands to use the door, either side of it: across
    /// its thinner side, `away` units from its face, at its foot. Relative
    /// to `foot`.
    pub fn sides(&self, away: f32) -> [[f32; 3]; 2] {
        let size: [f32; 3] = std::array::from_fn(|axis| self.max[axis] - self.min[axis]);
        let across = usize::from(size[1] < size[0]);
        let reach = size[across] / 2.0 + away;
        let at = |sign: f32| {
            let mut point = [0.0; 3];
            point[across] = sign * reach;
            point
        };
        [at(-1.0), at(1.0)]
    }
}

/// The direction an entity moves along: its `angles` (pitch, yaw and
/// roll), else its `angle` (a yaw; -1 is up and -2 is down).
fn direction(entity: &Entity) -> [f32; 3] {
    let number = |key: &str| entity.get(key)?.trim().parse::<f32>().ok();
    let angles = entity
        .vector("angles")
        .unwrap_or([0.0, number("angle").unwrap_or(0.0), 0.0]);
    if angles == [0.0, -1.0, 0.0] {
        return [0.0, 0.0, 1.0];
    }
    if angles == [0.0, -2.0, 0.0] {
        return [0.0, 0.0, -1.0];
    }
    collision::trig::angle_vectors(angles).0
}

/// Whether model `index` is wide and flat enough to stand on.
pub fn is_platform(bsp: &Bsp, index: usize) -> bool {
    bsp.models.get(index).is_some_and(|model| {
        let size: [f32; 3] = std::array::from_fn(|axis| model.max[axis] - model.min[axis]);
        let narrow = size[0].min(size[1]);
        narrow >= PLATFORM_WIDE && size[2] * PLATFORM_FLAT <= narrow
    })
}

/// The level's doors and lifts, in the order of their first entity.
pub fn movers(bsp: &Bsp) -> Vec<Mover> {
    let mut movers: Vec<Mover> = Vec::new();
    // The team each mover was made for, to add its other parts to.
    let mut teams: Vec<Option<String>> = Vec::new();
    for entity in &bsp.entities {
        let class = entity.class();
        if class != "func_door" && class != "func_plat" {
            continue;
        }
        let Some(index) = entity.model().filter(|index| *index > 0) else {
            continue;
        };
        let Some(model) = bsp.models.get(index) else {
            continue;
        };
        let number = |key: &str| entity.get(key)?.trim().parse::<f32>().ok();
        let way = direction(entity);
        let size: [f32; 3] = std::array::from_fn(|axis| model.max[axis] - model.min[axis]);
        let along: f32 = (0..3).map(|axis| way[axis].abs() * size[axis]).sum();
        let distance = (along - number("lip").unwrap_or(LIP)).max(0.0);
        let speed = number("speed")
            .filter(|speed| *speed > 0.0)
            .unwrap_or(SPEED);
        let moves = way.map(|part| part * distance);
        let kind = if class == "func_plat" || (way[2].abs() > 0.7 && is_platform(bsp, index)) {
            Kind::Lift
        } else {
            Kind::Door
        };
        let team = entity.get("team").filter(|team| !team.is_empty());
        let joined = team.and_then(|team| {
            (teams.iter())
                .position(|other| other.as_deref() == Some(team))
                .filter(|&at| movers[at].kind == kind)
        });
        let flags = entity
            .get("spawnflags")
            .and_then(|flags| flags.trim().parse::<i64>().ok())
            .unwrap_or(0);
        match joined {
            Some(at) => {
                let mover = &mut movers[at];
                mover.parts.push((index, moves));
                mover.seconds = mover.seconds.max(distance / speed);
            }
            None => {
                movers.push(Mover {
                    kind,
                    name: entity.get("targetname").unwrap_or("").to_string(),
                    parts: vec![(index, moves)],
                    seconds: distance / speed,
                    starts_open: flags & START_OPEN != 0,
                    middle: [0.0; 3],
                    min: [0.0; 3],
                    max: [0.0; 3],
                });
                teams.push(team.map(str::to_string));
            }
        }
    }
    for mover in &mut movers {
        let models = mover.parts.iter().map(|(index, _)| &bsp.models[*index]);
        let (min, max) = models.fold(
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
            |(min, max), model| {
                (
                    std::array::from_fn(|axis| min[axis].min(model.min[axis])),
                    std::array::from_fn(|axis| max[axis].max(model.max[axis])),
                )
            },
        );
        mover.middle = std::array::from_fn(|axis| (min[axis] + max[axis]) / 2.0);
        (mover.min, mover.max) = (min, max);
    }
    movers
}

/// The level's doors, without its lifts. A door's place here is its
/// number: the object it is in a kjk module, and the owner of its brushes
/// in the level's collision.
pub fn doors(movers: &[Mover]) -> Vec<&Mover> {
    (movers.iter())
        .filter(|mover| mover.kind == Kind::Door)
        .collect()
}

/// The door each part belongs to: for each of the level's models that is
/// part of a door (not a lift), the door's number (`doors`) and the part's
/// place in the door.
pub fn door_parts(movers: &[Mover]) -> std::collections::BTreeMap<usize, (usize, usize)> {
    let mut parts = std::collections::BTreeMap::new();
    for (door, mover) in doors(movers).into_iter().enumerate() {
        for (part, (index, _)) in mover.parts.iter().enumerate() {
            parts.insert(*index, (door, part));
        }
    }
    parts
}
