//! What each bolt hit on a body was, as the view saw it, and which of those
//! hits leave a burn.
//!
//! The owner ruled which bolts burn. A bolt leaves a burn when it is a
//! critical hit, a Power Blast, a Sniper Shot or the killing shot. A bolt
//! also leaves a burn when it deals at least 15% of the target's maximum
//! vitality. Any other bolt leaves a burn only when the body's last bolt
//! burn was laid at least 500 ms before, so a rapid stream of light hits
//! leaves at most two burns a second. A body keeps at most 6 bolt burns, and
//! a seventh replaces the oldest bolt burn. Bolt burns never push out
//! lightsaber burns. A hit on a bare head never leaves a burn, and no burn
//! may reach a bare head's mesh. The owner ruled that a head of armour or
//! metal takes burns like the rest of the body. That is a droid's head, and
//! a head that one of the body's models shows as a helmet or a full mask
//! (`ARMOURED_HEADS`). A burn is small for a pistol, larger for a rifle
//! and largest for a Power Blast. The light flash, the smoke and the flinch
//! play on every bolt hit, whether or not it leaves a burn.
//!
//! The 500 ms gate is measured in snapshot time, so two machines and a
//! replay agree on it.
//!
//! A hit counts as the killing shot when the body has no vitality left in
//! the snapshot that brings the hit. Every bolt hit seen in the snapshot
//! where a body dies therefore counts as the killing shot.
//!
//! `combat_fx` writes the records. It keeps a trail of every bolt it draws,
//! and a hit is matched to the trail of the shooter's bolt that passes
//! nearest the hit location (`pick_trail`). The record's line is that
//! bolt's line as drawn (`line_of`). `body_marks` lays the burns.

use bevy::prelude::*;
use sim::SimId;
use sim::combat::Location;

/// A hit dealing at least this percentage of the target's maximum vitality
/// always leaves a burn.
pub(crate) const HEAVY_PERCENT: i32 = 15;
/// The least time between two bolt burns on one body (ms of snapshot time),
/// unless the second hit always burns.
pub(crate) const BURN_GAP_MS: u64 = 500;
/// The most bolt burns kept on one body. A new one past these replaces the
/// oldest bolt burn.
pub(crate) const MOST_BOLT_BURNS: usize = 6;
/// The models, in both games, whose head is a helmet or a full mask with no
/// face showing: the Sith trooper's (and its lighter copy), the
/// Mandalorian's, Mandalore's, and the head of KOTOR 2's Sith assassin. A
/// Republic soldier's helmet leaves the face bare, and so does a swoop gang
/// member's hood, so neither is here.
pub(crate) const ARMOURED_HEADS: [&str; 5] = [
    "n_sithsoldier",
    "l_sithsoldier",
    "n_mandalorian",
    "p_mandalorebb",
    "n_sithassnh",
];

/// Whether the model `name` shows its head as a helmet or a full mask
/// (`ARMOURED_HEADS`), whatever its case.
pub(crate) fn armoured_head(name: &str) -> bool {
    ARMOURED_HEADS
        .iter()
        .any(|armoured| armoured.eq_ignore_ascii_case(name))
}
/// How long a record of a bolt hit is kept (ms), so that a system that runs
/// before the records are written in a frame still sees them in the next.
pub(crate) const KEPT_MS: u64 = 350;

/// How long a bolt's trail is kept after the bolt was last drawn (ms).
pub(crate) const TRAIL_MS: f32 = 500.0;
/// A hit is matched only to a bolt drawn within this long of it (ms).
pub(crate) const TRAIL_MATCH_MS: f32 = 250.0;
/// A hit is matched only to a bolt whose line passes this near the hit
/// location's node (metres).
pub(crate) const TRAIL_NEAR_M: f32 = 1.5;

/// A bolt as the view drew it, kept for `TRAIL_MS` after it was last drawn.
/// Its line is the one `combat_fx` draws the bolt along (`line_of`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trail {
    /// Who fired it.
    pub owner: SimId,
    /// Where it started (game space, units), and its velocity (units per
    /// second).
    pub base: Vec3,
    pub velocity: Vec3,
    /// How far off its way it left the gun as drawn (units).
    pub launch: Vec3,
    /// When it was fired, and when it was last drawn (the view's ms).
    pub fired: f32,
    pub last_ms: f32,
    /// The gun that fired it.
    pub weapon: sim::combat::sheet::Weapon,
    /// Whether it is a Power Blast's bolt, or a Sniper Shot's.
    pub power: bool,
    pub sniper: bool,
}

/// The trail of `by`'s bolt that hit at `ms` (the view's) near `node`
/// (game space, units). Of `by`'s bolts drawn within `TRAIL_MATCH_MS` of the
/// hit whose lines pass within `TRAIL_NEAR_M` of the node, it is the one
/// whose line passes nearest. A bolt that is already past the node at the
/// hit's time, by more than the distance it flies in one tick, missed and
/// flew on, so it is never picked.
pub(crate) fn pick_trail(trails: &[Trail], by: SimId, ms: f32, node: Vec3) -> Option<&Trail> {
    let tick_s = 1.0 / sim::TICK_HZ as f32;
    trails
        .iter()
        .filter(|trail| trail.owner == by && (ms - trail.last_ms).abs() <= TRAIL_MATCH_MS)
        .filter_map(|trail| {
            let (at, way) = line_of(trail, ms);
            let across = node - at;
            let ahead = across.dot(way);
            let one_tick = trail.velocity.length() * tick_s;
            let off = (across - way * ahead).length() * asset_ja::UNIT;
            (ahead >= -one_tick).then_some((trail, off))
        })
        .filter(|&(_, off)| off < TRAIL_NEAR_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(trail, _)| trail)
}

/// The line a trail was drawn along when it met the body at `ms` (the
/// view's): its point then (game space, units) and its way (a unit vector).
pub(crate) fn line_of(trail: &Trail, ms: f32) -> (Vec3, Vec3) {
    crate::combat_fx::drawn_line((trail.base, trail.velocity, trail.launch), ms - trail.fired)
}

/// The point of the line through `from` along `way` (a unit vector) that
/// lies nearest `point`, in the units the three are given in. This is where
/// a bolt's line passes the body's hit location, so it lies on the body even
/// when `from` is the bolt as drawn a little past the body.
pub(crate) fn nearest_on_line((from, way): (Vec3, Vec3), point: Vec3) -> Vec3 {
    from + way * (point - from).dot(way)
}

/// How large a bolt burn is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BurnSize {
    /// A bolt from a one-handed gun.
    Pistol,
    /// A bolt from a two-handed gun.
    Rifle,
    /// A Power Blast, from either.
    PowerBlast,
}

/// What one bolt hit was, as the view saw it.
#[derive(Clone, Copy, Debug)]
pub struct BoltHit {
    /// The body struck.
    pub victim: SimId,
    /// The shooter, when the event names one.
    pub by: Option<SimId>,
    /// The event's number in the victim's ring.
    pub sequence: u16,
    /// The event's snapshot time (ms).
    pub ms: u64,
    /// Where on the body the bolt hit.
    pub location: Location,
    pub critical: bool,
    pub power_blast: bool,
    pub sniper_shot: bool,
    /// Whether the body had no vitality left in the snapshot of this hit.
    /// Every bolt hit seen in the snapshot where a body dies counts as the
    /// killing shot, so two bolts that hit in that snapshot both burn.
    pub killing: bool,
    /// The damage dealt, in hundredths of a point of vitality, as the event
    /// has it.
    pub damage: i32,
    /// The body's maximum vitality, in whole points (`Sheet::max_hp`).
    pub max_hp: u16,
    /// Whether the gun that fired the bolt is held in one hand
    /// (`Weapon::one_handed`). It is true when neither the bolt's trail nor
    /// the attacker names a gun, so an unknown gun leaves the smallest burn.
    pub one_handed: bool,
    /// A point on the bolt's line as drawn (game space, metres).
    pub from: Vec3,
    /// The bolt's way along that line (a unit vector).
    pub way: Vec3,
    /// The hit location's node as posed (game space, metres), when the body
    /// has one.
    pub node: Option<Vec3>,
    /// The colour of the gun's flash (`weapons::Handling::flash_tint`).
    pub tint: Vec3,
    /// Whether the body's head is armour or metal (`cast::Hitboxes::1`), so
    /// that a hit there may burn.
    pub armoured_head: bool,
}

/// The bolt hits of the last `KEPT_MS`, oldest first.
#[derive(Resource, Default)]
pub struct BoltHits(pub Vec<BoltHit>);

/// Whether `hit` leaves a burn, given when the body's last bolt burn was
/// laid (snapshot ms), or `None` when it has none. A hit on a bare head
/// never burns. A hit on an armoured head burns as a hit anywhere else does.
pub(crate) fn burns(hit: &BoltHit, last_burn_ms: Option<u64>) -> bool {
    if hit.location == Location::Head && !hit.armoured_head {
        return false;
    }
    always_burns(hit) || last_burn_ms.is_none_or(|last| hit.ms.saturating_sub(last) >= BURN_GAP_MS)
}

/// Whether `hit` is heavy enough to leave a burn whatever the gate says. The
/// damage is in hundredths of a point and the maximum vitality in whole
/// points, so 15% of it is `HEAVY_PERCENT` times the maximum in hundredths.
/// A body with no maximum vitality counts no hit as heavy by damage.
pub(crate) fn always_burns(hit: &BoltHit) -> bool {
    let heavy = hit.max_hp > 0 && hit.damage >= HEAVY_PERCENT * i32::from(hit.max_hp);
    hit.critical || hit.power_blast || hit.sniper_shot || hit.killing || heavy
}

/// How large the burn of a bolt from a gun held in one hand (`one_handed`)
/// or two is, or of a Power Blast.
pub(crate) fn burn_size(one_handed: bool, power_blast: bool) -> BurnSize {
    match (power_blast, one_handed) {
        (true, _) => BurnSize::PowerBlast,
        (false, true) => BurnSize::Pistol,
        (false, false) => BurnSize::Rifle,
    }
}

/// How far a bolt hit turns its node at the most (degrees), and how soon
/// after the hit it gets there (ms).
pub(crate) const FLINCH_DEG: f32 = 6.0;
pub(crate) const FLINCH_RISE_MS: f32 = 40.0;
/// How long after the hit the node is back where it was (ms).
pub(crate) const FLINCH_MS: f32 = 300.0;
/// The most that several flinches added up turn one node (degrees).
pub(crate) const FLINCH_MOST_DEG: f32 = 12.0;
/// How much of a flinch lower on the trunk the upper torso takes too.
pub(crate) const TRUNK_SHARE: f32 = 0.4;

/// How far a flinch has turned the node `age` ms after the hit (degrees).
/// The turn rises evenly to `FLINCH_DEG` at `FLINCH_RISE_MS`, then eases back
/// along half a cosine to nothing at `FLINCH_MS`.
pub(crate) fn flinch_deg(age: f32) -> f32 {
    if age <= 0.0 || age >= FLINCH_MS {
        0.0
    } else if age < FLINCH_RISE_MS {
        FLINCH_DEG * age / FLINCH_RISE_MS
    } else {
        let back = (age - FLINCH_RISE_MS) / (FLINCH_MS - FLINCH_RISE_MS);
        FLINCH_DEG * 0.5 * (1.0 + (std::f32::consts::PI * back).cos())
    }
}

/// The turn of a node for the flinches of `hits` at `now`. Each hit is a
/// bolt's way and the ms it hit. The node turns about `up × way`, so its
/// top (+Z) moves along each bolt's way. The turns add up, to at most
/// `FLINCH_MOST_DEG`. The view adds the upper torso's share to its own
/// flinches first (`cast::bolt_flinch`), so it caps `flinch_axis` itself.
#[cfg_attr(not(feature = "probes"), expect(dead_code))]
pub(crate) fn flinch_turn(hits: &[(Vec3, f32)], now: f32) -> Quat {
    capped_turn(flinch_axis(hits, now))
}

/// The flinches of `hits` at `now` added up, before the cap: the axis of the
/// turn, as long as the turn's angle (radians).
pub(crate) fn flinch_axis(hits: &[(Vec3, f32)], now: f32) -> Vec3 {
    hits.iter()
        .map(|&(way, ms)| {
            Vec3::Z.cross(way).normalize_or_zero() * flinch_deg(now - ms).to_radians()
        })
        .sum()
}

/// The turn about `axis` by its length (radians), capped at
/// `FLINCH_MOST_DEG`.
pub(crate) fn capped_turn(axis: Vec3) -> Quat {
    Quat::from_scaled_axis(axis.clamp_length_max(FLINCH_MOST_DEG.to_radians()))
}

/// The way a killing blow throws a body, made level. A bolt's own way
/// (`bolt_way`) comes first. Without one, the body is thrown away from its
/// striker (`from_striker`, the body's place less the striker's), or else
/// along its own velocity. None when none of them is level enough to give a
/// way.
pub(crate) fn blow_way(
    bolt_way: Option<Vec3>,
    from_striker: Option<Vec3>,
    body_velocity: Vec3,
) -> Option<Vec3> {
    let flat = |way: Vec3| way.with_z(0.0).try_normalize();
    bolt_way
        .and_then(flat)
        .or_else(|| from_striker.and_then(flat))
        .or_else(|| flat(body_velocity))
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
