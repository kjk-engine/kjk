//! A blade's sweep through a tick, and what an attack owes the bodies it
//! cuts.
//!
//! The blade moves from where it was (its muzzle and direction) to where
//! it is now; the sweep is cut into straight traces every `STEP` units up
//! the blade, from each point's old place to its new one. A wide turn is
//! cut into chunks first: the direction turns by its angles the short way
//! round while the muzzle moves straight, so a fast swing still sweeps its
//! arc (`segments`). Two blades clash when their sweeps meet: each sweep,
//! reached a little past the blade and pushed a little ahead, makes two
//! triangles (`sweep_quad`), tested edge against triangle (`quads_meet`).
//!
//! An attack (`Swing`) keeps count of the bodies it has cut (`Contact`):
//! the sweep through each that is not yet delivered, the budget delivered
//! so far, and whether that body's blade turned the swing. As the blade
//! leaves a body (or the attack ends), the body is owed its share of a
//! budget by how much of `NOMINAL` it was swept through, at least
//! `LEAST_SHARE` the first time and never past one budget (`Delivery`).

use collision::{Bodies, Body, BoxWorld, CollisionWorld, Contents, Extents, UNIT};

use crate::combat::damage;
use crate::math::{self, Vec3, angle_vectors, atan2_deg};
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

use super::Move;
use super::blades::normalize_180;

/// Traces every this many units up the blade.
pub const STEP: f32 = rules::sabers::sweep::STEP;
/// The clash test's reach past the blade.
pub const EXTRAPOLATE: f32 = rules::sabers::sweep::EXTRAPOLATE;
/// A turn with a dot of the directions below 1 less this is cut into
/// chunks.
const ARC_STEP: f32 = rules::sabers::sweep::ARC_STEP;
/// The sweep through a body a whole budget is delivered for (units of
/// `Segment::worth` summed).
pub const NOMINAL: f32 = rules::sabers::sweep::NOMINAL;
/// The least share of a budget a blade that cuts a body delivers to it
/// (thousandths).
pub const LEAST_SHARE: u32 = rules::sabers::sweep::LEAST_SHARE;

/// One trace of a sweep (units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: Vec3,
    pub to: Vec3,
    /// The point's whole travel this chunk (`L`).
    pub length: f32,
}

impl Segment {
    /// What a trace that entered at `fraction` of the way is worth.
    pub fn worth(&self, fraction: f32) -> f32 {
        self.length * (1.0 - fraction)
    }

    /// The point `fraction` of the way along it.
    pub fn at(&self, fraction: f32) -> Vec3 {
        math::add(
            self.from,
            math::scale(math::sub(self.to, self.from), fraction),
        )
    }
}

/// The pitch and yaw a direction points along, as the sweep turns it: the
/// pitch is "0 minus" the climb so that a level blade's is `+0.0`, and
/// neither is wrapped onto the circle.
fn pointing(d: Vec3) -> [f32; 3] {
    let yaw = if d[0] == 0.0 && d[1] == 0.0 {
        0.0
    } else {
        atan2_deg(d[1], d[0])
    };
    [0.0 - atan2_deg(d[2], math::flat_length(d)), yaw, 0.0]
}

/// `t` of the way from angle `a` to `b`, the short way round.
fn toward(a: f32, mut b: f32, t: f32) -> f32 {
    if b - a > 180.0 {
        b -= 360.0;
    }
    if b - a < -180.0 {
        b += 360.0;
    }
    a + t * (b - a)
}

/// A blade `length` long swept from muzzle `b0` pointing `d0` to `b1`
/// pointing `d1` (unit directions), cut into traces every `STEP` up the
/// blade, in order. A turn too wide for one chunk (when `arcs`) is cut
/// into chunks of at most `ARC_STEP` each; a blade that did not move
/// sweeps nothing.
pub fn segments(b0: Vec3, d0: Vec3, b1: Vec3, d1: Vec3, length: f32, arcs: bool) -> Vec<Segment> {
    let mut traces = Vec::new();
    if b0 == b1 && d0 == d1 {
        return traces;
    }
    let dot = if arcs { math::dot(d0, d1) } else { 1.0 };
    let (step, mut at) = if dot < 1.0 - ARC_STEP {
        let step = ARC_STEP / (1.0 - dot);
        (step, step)
    } else {
        (0.0, 1.0)
    };
    let (from_angles, to_angles) = (pointing(d0), pointing(d1));
    let (mut base, mut direction) = (b0, d0);
    loop {
        let (far_base, far_direction) = if at >= 1.0 {
            (b1, d1)
        } else {
            let turned = [0, 1, 2].map(|axis| toward(from_angles[axis], to_angles[axis], at));
            (
                math::mad(b0, at, math::sub(b1, b0)),
                angle_vectors(turned).0,
            )
        };
        let mut up = STEP;
        while up <= length {
            let from = math::mad(base, up, direction);
            let to = math::mad(far_base, up, far_direction);
            traces.push(Segment {
                from,
                to,
                length: math::length(math::sub(to, from)),
            });
            up += STEP;
        }
        if at >= 1.0 {
            return traces;
        }
        (base, direction) = (far_base, far_direction);
        at = (at + step).min(1.0);
    }
}

/// How far along `segment` (units) a box `half` units each way first meets
/// `body` (metres), if it does: 0 when it starts inside.
pub fn enters(segment: &Segment, body: &Body, half: f32) -> Option<f32> {
    let h = half * UNIT;
    let extents = Extents {
        mins: [-h; 3],
        maxs: [h; 3],
    };
    let trace = Bodies::new(&BoxWorld::new(), std::slice::from_ref(body)).trace(
        math::scale(segment.from, UNIT),
        math::scale(segment.to, UNIT),
        &extents,
        Contents::BODY,
    );
    trace.hit().then_some(if trace.start_solid {
        0.0
    } else {
        trace.fraction
    })
}

/// The clash test's shape of a blade's sweep: two triangles from the old
/// muzzle, through the old tip reached `EXTRAPOLATE` past the blade, to
/// the new tip and the new muzzle, each pushed `EXTRAPOLATE` further along
/// its own motion.
pub fn sweep_quad(b0: Vec3, d0: Vec3, b1: Vec3, d1: Vec3, length: f32) -> [[Vec3; 3]; 2] {
    let reach = length + EXTRAPOLATE;
    let old_tip = math::mad(b0, reach, d0);
    let pushed = |old: Vec3, new: Vec3| {
        let mut way = math::sub(new, old);
        math::normalize(&mut way);
        math::mad(new, EXTRAPOLATE, way)
    };
    let new_base = pushed(b0, b1);
    let new_tip = pushed(math::mad(b0, length, d0), math::mad(b1, length, d1));
    [[b0, old_tip, new_tip], [b0, new_tip, new_base]]
}

/// Where the segment `p`→`q` crosses the triangle, if it does (a
/// degenerate triangle is met by nothing).
fn segment_meets(p: Vec3, q: Vec3, tri: &[Vec3; 3]) -> bool {
    let e1 = math::sub(tri[1], tri[0]);
    let e2 = math::sub(tri[2], tri[0]);
    let d = math::sub(q, p);
    let h = math::cross(d, e2);
    let det = math::dot(e1, h);
    if det == 0.0 {
        return false;
    }
    let inv = 1.0 / det;
    let s = math::sub(p, tri[0]);
    let u = math::dot(s, h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let qv = math::cross(s, e1);
    let v = math::dot(d, qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = math::dot(e2, qv) * inv;
    (0.0..=1.0).contains(&t)
}

/// Whether two triangles meet: an edge of one crosses the other.
pub fn triangles_meet(a: &[Vec3; 3], b: &[Vec3; 3]) -> bool {
    let crosses = |edges: &[Vec3; 3], tri: &[Vec3; 3]| {
        (0..3).any(|i| segment_meets(edges[i], edges[(i + 1) % 3], tri))
    };
    crosses(a, b) || crosses(b, a)
}

/// Whether two blades' sweeps meet (the four triangle pairs).
pub fn quads_meet(a: &[[Vec3; 3]; 2], b: &[[Vec3; 3]; 2]) -> bool {
    a.iter().any(|x| b.iter().any(|y| triangles_meet(x, y)))
}

/// A body an attack is cutting, or has cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub victim: SimId,
    /// The sweep through it not yet delivered (units, `Segment::worth`).
    pub pending: f32,
    /// The budget delivered to it so far this move, thousandths.
    pub delivered: u16,
    /// When a trace was last in it (sim ms).
    pub touched_ms: u64,
    /// Where the contact began and which way the blade moved (units).
    pub point: Vec3,
    pub direction: Vec3,
    /// Its blade turned the swing (`Swing::turn`).
    pub turned: bool,
    /// Its blade's chance to turn the swing was drawn (`Swing::turn`,
    /// `Swing::pass`): a body only cut has had no draw.
    pub drawn: bool,
}

impl Contact {
    /// The share of a budget its pending sweep comes to now (thousandths:
    /// at least `LEAST_SHARE` the first time, one after, no more than what
    /// is left of one budget), if any.
    pub fn due_share(&self) -> Option<u16> {
        if self.pending <= 0.0 {
            return None;
        }
        let least = if self.delivered == 0 { LEAST_SHARE } else { 1 };
        let share = ((self.pending / NOMINAL * 1000.0) as u32)
            .clamp(least, 1000)
            .min(u32::from(1000 - self.delivered)) as u16;
        (share > 0).then_some(share)
    }
}

/// Most bodies one attack keeps count of (JA: 15 a frame; four in one
/// swing is plenty for the sim's crowds).
pub const CONTACTS: usize = 4;

/// The attack under way and what it owes (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Swing {
    /// The move, and the torso's restart bit when it began (the same move
    /// started over is a new swing).
    pub saber_move: Move,
    pub flip: bool,
    /// Its cadence (C2, ms; `begin`).
    pub cadence_ms: u32,
    /// When its holder's latest attack began (sim ms; 0 before any).
    pub struck_ms: u64,
    pub contacts: [Option<Contact>; CONTACTS],
}

/// A delivery: `victim` takes `share` thousandths of a roll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Delivery {
    pub victim: SimId,
    pub share: u16,
    pub point: Vec3,
    pub direction: Vec3,
    pub cadence_ms: u32,
}

impl Swing {
    /// The swing of `saber_move` begun at `now` after `last`, its move held
    /// `hold_ms`. An attack's cadence is the time since its holder's last
    /// attack began (the rhythm the blows truly come at: a chain's swings
    /// are a third of a second each, but return and transition between), no
    /// less than its hold and no more than a whole roll's
    /// (`damage::WHOLE_CADENCE_MS`, as is a first attack); any other move's
    /// is its hold.
    pub fn begin(last: &Swing, saber_move: Move, flip: bool, hold_ms: u32, now: u64) -> Swing {
        let (cadence_ms, struck_ms) = if saber_move.attack() {
            let since = match last.struck_ms {
                0 => u64::MAX,
                then => now.saturating_sub(then),
            };
            let whole = u64::from(damage::WHOLE_CADENCE_MS);
            (since.min(whole).max(u64::from(hold_ms)) as u32, now)
        } else {
            (hold_ms, last.struck_ms)
        };
        Swing {
            saber_move,
            flip,
            cadence_ms,
            struck_ms,
            contacts: [None; CONTACTS],
        }
    }

    /// Whether this is the swing of `saber_move` begun with the torso's
    /// restart bit `flip`.
    pub fn is(&self, saber_move: Move, flip: bool) -> bool {
        self.saber_move == saber_move && self.flip == flip
    }

    /// `victim`'s contact, made at `point` along `direction` at `now` if it
    /// has none and there is room (a fifth body is not counted).
    fn contact(
        &mut self,
        victim: SimId,
        point: Vec3,
        direction: Vec3,
        now: u64,
    ) -> Option<&mut Contact> {
        let at = (self.contacts.iter())
            .position(|c| c.is_some_and(|c| c.victim == victim))
            .or_else(|| self.contacts.iter().position(Option::is_none))?;
        Some(self.contacts[at].get_or_insert(Contact {
            victim,
            pending: 0.0,
            delivered: 0,
            touched_ms: now,
            point,
            direction,
            turned: false,
            drawn: false,
        }))
    }

    /// A trace of this swing in `victim` at `now`, worth `worth`, entering
    /// at `point` moving along `direction`; nothing on a body that turned it.
    pub fn touch(&mut self, victim: SimId, worth: f32, point: Vec3, direction: Vec3, now: u64) {
        if let Some(contact) = self.contact(victim, point, direction, now)
            && !contact.turned
        {
            if contact.pending == 0.0 {
                contact.point = point;
                contact.direction = direction;
            }
            contact.pending += worth;
            contact.touched_ms = now;
        }
    }

    /// `victim`'s blade turned this swing at `point`: it owes that body
    /// nothing.
    pub fn turn(&mut self, victim: SimId, point: Vec3, now: u64) {
        if let Some(contact) = self.contact(victim, point, [0.0; 3], now) {
            contact.turned = true;
            contact.drawn = true;
            contact.pending = 0.0;
        }
    }

    /// `victim`'s blade would have turned this swing at `point`, but the
    /// swing got past it: a touch worth nothing, the draw kept.
    pub fn pass(&mut self, victim: SimId, point: Vec3, now: u64) {
        self.touch(victim, 0.0, point, [0.0; 3], now);
        if let Some(contact) = self.contact(victim, point, [0.0; 3], now) {
            contact.drawn = true;
        }
    }

    /// Whether `victim` turned this swing (`Some(true)`) or it got past
    /// (`Some(false)`); none while no draw was made, though the blade may
    /// have cut it.
    pub fn verdict(&self, victim: SimId) -> Option<bool> {
        (self.contacts.iter().flatten())
            .find(|c| c.victim == victim && c.drawn)
            .map(|c| c.turned)
    }

    /// Forgets `victim`'s pending sweep (it blocked).
    pub fn drop_pending(&mut self, victim: SimId) {
        for contact in self.contacts.iter_mut().flatten() {
            if contact.victim == victim {
                contact.pending = 0.0;
            }
        }
    }

    /// The deliveries due at `now`: every contact the blade left (or all
    /// of them when `ending`), at the share its sweep is of `NOMINAL`, up to
    /// what is left of one budget.
    pub fn due(&mut self, now: u64, ending: bool) -> Vec<Delivery> {
        let cadence_ms = self.cadence_ms;
        let mut out = Vec::new();
        for contact in self.contacts.iter_mut().flatten() {
            if !ending && contact.touched_ms >= now {
                continue;
            }
            let due = contact.due_share();
            contact.pending = 0.0;
            let Some(share) = due else {
                continue;
            };
            contact.delivered += share;
            out.push(Delivery {
                victim: contact.victim,
                share,
                point: contact.point,
                direction: contact.direction,
                cadence_ms,
            });
        }
        out
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.saber_move as u8);
        out.u8(u8::from(self.flip));
        out.u32(self.cadence_ms);
        out.u64(self.struck_ms);
        for contact in &self.contacts {
            write_fixed(out, contact.as_ref(), &BLANK_CONTACT, |c, out| {
                out.u32(c.victim.0);
                out.f32(c.pending);
                out.u16(c.delivered);
                out.u64(c.touched_ms);
                for value in c.point.iter().chain(&c.direction) {
                    out.f32(*value);
                }
                out.u8(u8::from(c.turned));
                out.u8(u8::from(c.drawn));
            });
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Swing, DecodeError> {
        let at = input.at();
        let saber_move =
            Move::from_index(u16::from(input.u8()?)).ok_or(input.invalid("swing move", at))?;
        let at = input.at();
        let flip = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("swing flip", at)),
        };
        let cadence_ms = input.u32()?;
        let struck_ms = input.u64()?;
        let mut contacts = [None; CONTACTS];
        for slot in &mut contacts {
            *slot = read_fixed(
                input,
                &BLANK_CONTACT,
                |input| {
                    let victim = SimId(input.u32()?);
                    let pending = input.f32()?;
                    let delivered = input.u16()?;
                    let touched_ms = input.u64()?;
                    let mut values = [0.0f32; 6];
                    for value in &mut values {
                        *value = input.f32()?;
                    }
                    let turned = input.u8()? != 0;
                    let drawn = input.u8()? != 0;
                    Ok(Contact {
                        victim,
                        pending,
                        delivered,
                        touched_ms,
                        point: [values[0], values[1], values[2]],
                        direction: [values[3], values[4], values[5]],
                        turned,
                        drawn,
                    })
                },
                "contact flag",
            )?;
        }
        Ok(Swing {
            saber_move,
            flip,
            cadence_ms,
            struck_ms,
            contacts,
        })
    }
}

const BLANK_CONTACT: Contact = Contact {
    victim: SimId(0),
    pending: 0.0,
    delivered: 0,
    touched_ms: 0,
    point: [0.0; 3],
    direction: [0.0; 3],
    turned: false,
    drawn: false,
};

/// How far `p` is from the segment `a`→`b`.
pub fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    math::length(math::sub(p, closest_on_segment(p, a, b)))
}

/// The point of the segment `a`–`b` nearest `p`.
pub fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = math::sub(b, a);
    let length = math::dot(ab, ab);
    let t = if length > 0.0 {
        (math::dot(math::sub(p, a), ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    math::mad(a, t, ab)
}

/// The yaw from `from` to `to` in degrees, (−180, 180].
pub fn yaw_to(from: Vec3, to: Vec3) -> f32 {
    normalize_180(atan2_deg(to[1] - from[1], to[0] - from[0]))
}
