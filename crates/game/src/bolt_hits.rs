//! What each bolt hit on a body was, as the view saw it, and which of those
//! hits leave a burn.
//!
//! The owner ruled which bolts burn. A bolt leaves a burn when it is a
//! critical hit, a Power Blast, a Sniper Shot or the killing shot. A bolt
//! also leaves a burn when it deals at least 15% of the target's maximum
//! vitality. Any other bolt leaves a burn only when the body's last bolt
//! burn was laid at least 500 ms before, so a rapid stream of light hits
//! leaves at most two burns a second. A hit on the head never leaves a burn.
//! A burn is small for a pistol, larger for a rifle and largest for a Power
//! Blast. The light flash, the smoke and the flinch play on every bolt hit,
//! whether or not it leaves a burn.
//!
//! The 500 ms gate is measured in snapshot time, so two machines and a
//! replay agree on it.
#![cfg_attr(
    not(feature = "probes"),
    expect(
        dead_code,
        reason = "only the probes read the burn rule until the burns are laid"
    )
)]

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
#[expect(
    dead_code,
    reason = "the bolt burns on bodies are laid in a later change"
)]
pub(crate) const MOST_BOLT_BURNS: usize = 6;
/// How long a record of a bolt hit is kept (ms), so that a system that runs
/// before the records are written in a frame still sees them in the next.
#[expect(dead_code, reason = "the records are written in a later change")]
pub(crate) const KEPT_MS: u64 = 350;

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
    /// Whether this hit left the body with no vitality.
    pub killing: bool,
    /// The damage dealt, in hundredths of a point of vitality, as the event
    /// has it.
    pub damage: i32,
    /// The body's maximum vitality, in whole points (`Sheet::max_hp`).
    pub max_hp: u16,
    /// Whether the gun that fired the bolt is held in one hand
    /// (`Weapon::one_handed`).
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
}

/// The bolt hits of the last `KEPT_MS`, oldest first.
#[derive(Resource, Default)]
pub struct BoltHits(pub Vec<BoltHit>);

/// Whether `hit` leaves a burn, given when the body's last bolt burn was
/// laid (snapshot ms), or `None` when it has none.
pub(crate) fn burns(hit: &BoltHit, last_burn_ms: Option<u64>) -> bool {
    if hit.location == Location::Head {
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

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
