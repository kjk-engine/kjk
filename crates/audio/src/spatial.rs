//! Where a sound is heard from: how loud it is in each ear, given who
//! listens and where the sound is. Distances are in the game's units and
//! the numbers are Jedi Academy's volumes: a sound plays at full volume up
//! to a distance set by its kind, then fades by a fixed share per unit;
//! each ear hears half of it whatever the side and the other half by how
//! far the sound is to that side; and the volume comes out in whole steps
//! of 1/255. A KOTOR sound object instead fades straight from its near to
//! its far distance.

use glam::Vec3;

/// Within this distance a normal sound plays at full volume (units).
const FULL_RANGE: f32 = 256.0;
/// How much volume a sound loses for each unit past its full range.
const FADE: f32 = 0.0008;
/// The same for a voice that fades quickly.
const FADE_NEAR_VOICE: f32 = 0.004;
/// Half of each ear's weight is the same on both sides; the other half
/// follows which side the sound is on.
const EAR_SHARE: f32 = 0.5;
/// Both ears' volume for a sound made by the listener's own character.
const OWN_SOUND: f32 = 0.75;
/// Volumes come in whole steps of one in this many.
const STEPS: f32 = 255.0;

/// Who hears: the eye, its right, and the character it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listener {
    pub origin: Vec3,
    pub right: Vec3,
    pub entity: Option<u32>,
}

impl Default for Listener {
    fn default() -> Listener {
        Listener {
            origin: Vec3::ZERO,
            right: Vec3::Y,
            entity: None,
        }
    }
}

/// How a sound falls off (the game's channels' rules, and KOTOR's sound
/// objects').
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Falloff {
    /// Most sounds.
    Normal,
    /// A sound that carries eight times as far before it fades.
    Far,
    /// A voice: full volume three times as far.
    Voice,
    /// A voice that fades quickly past 1.35 times as far.
    VoiceNear,
    /// Heard alike everywhere.
    Everywhere,
    /// Not placed: heard alike everywhere and in both ears alike (a
    /// conversation's voice, as reone and xoreos play it).
    Unplaced,
    /// A KOTOR sound object's: full within `near`, falling straight to
    /// nothing at `far` (units); `placed`, from where it is, else in both
    /// ears alike (xoreos's linear model, `docs/SOUND.md`).
    Range { near: f32, far: f32, placed: bool },
}

/// Where a sound is, and whose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub at: Vec3,
    pub entity: Option<u32>,
    pub falloff: Falloff,
}

/// One ear's weight for a sound at `side` (+1 right, -1 left): `ear` is +1
/// for the right ear, -1 for the left. Never below zero; above one when the
/// listener's right is longer than a unit.
fn ear(ear: f32, side: f32) -> f32 {
    (EAR_SHARE + (ear * EAR_SHARE) * side).max(0.0)
}

/// `volume` rounded down to a whole step, after losing `lost` of it.
fn stepped(lost: f32, volume: f32) -> f32 {
    (((1.0 - lost) * volume) * STEPS).max(0.0).floor() / STEPS
}

/// The volumes `(left, right)` at which `listener` hears `emitter`.
///
/// The listener's own character's sounds play at a fixed volume in both
/// ears, wherever they are. An unplaced sound is half volume in each ear. A
/// sound object's range fades straight from full at `near` to nothing at
/// `far` (a hard step at `far` when `far` is not past `near`), unstepped.
/// Every other sound keeps full volume to its kind's distance, then fades,
/// and is rounded down to a whole step.
pub fn gains(listener: &Listener, emitter: &Emitter) -> (f32, f32) {
    if emitter.entity.is_some() && emitter.entity == listener.entity {
        return (OWN_SOUND, OWN_SOUND);
    }
    let offset = emitter.at - listener.origin;
    let distance = offset.length();
    let side = listener.right.dot(offset.normalize_or_zero());
    let (full, fade) = match emitter.falloff {
        Falloff::Unplaced => return (ear(-1.0, 0.0), ear(1.0, 0.0)),
        Falloff::Range { near, far, placed } => {
            let span = (far - near).max(f32::EPSILON);
            let volume = ((far - distance) / span).clamp(0.0, 1.0);
            let side = if placed { side } else { 0.0 };
            return (ear(-1.0, side) * volume, ear(1.0, side) * volume);
        }
        Falloff::Normal => (FULL_RANGE, FADE),
        Falloff::Far => (FULL_RANGE * 8.0, FADE),
        Falloff::Voice => (FULL_RANGE * 3.0, FADE),
        Falloff::VoiceNear => (FULL_RANGE * 1.35, FADE_NEAR_VOICE),
        Falloff::Everywhere => (f32::INFINITY, 0.0),
    };
    let lost = (distance - full).max(0.0) * fade;
    (
        stepped(lost, ear(-1.0, side)),
        stepped(lost, ear(1.0, side)),
    )
}
