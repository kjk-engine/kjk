//! Stealth and Awareness (`docs/COMBAT.md`):
//! a skill's total, the rolls a creature holds, and whether an observer
//! spots or hears a stealthed one. Pure: the authority gathers the inputs
//! (`world::covert`, `world::scripts`, `world::minds`).

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;

use crate::combat::Sheet;
use crate::combat::sheet::Ability;
use crate::rng::SimRng;
use crate::script::Library;
use crate::wire::{DecodeError, Reader, Writer};
use rules::stealth::listen;
use rules::stealth::rolls;
use rules::stealth::spot;

pub const ROLL_EVERY_MS: u64 = rules::stealth::rolls::EVERY_MS;
pub const OPENER_MS: u64 = rules::stealth::mode::OPENER_MS;

/// How fast across the floor (units a second) a body may move and still
/// stand still, and must move to run: between them it walks.
const STILL_SPEED: f32 = rules::stealth::pace::STILL_SPEED;
const RUN_SPEED: f32 = rules::stealth::pace::RUN_SPEED;

/// Whether a body moving at `velocity` (units a second) stands still, and
/// whether it runs.
pub fn pace(velocity: [f32; 3]) -> (bool, bool) {
    let speed = (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt();
    (speed < STILL_SPEED, speed > RUN_SPEED)
}

/// The skills only those with ranks of their own may use (`skills.2da`
/// `untrained` 0: Demolitions, Stealth, Security, in both games).
const NEEDS_RANKS: [bool; 8] = [false, true, true, false, false, false, true, false];
/// Each skill's key ability where no library says (K1's `keyability`).
const KEY: [Ability; 8] = [
    Ability::Intelligence,
    Ability::Intelligence,
    Ability::Dexterity,
    Ability::Wisdom,
    Ability::Charisma,
    Ability::Intelligence,
    Ability::Wisdom,
    Ability::Wisdom,
];

/// Skill `skill`'s total: its ranks as effects and items leave them
/// with what feats add (`now`, `Sheet::skill`: Gear Head, Skill Focus,
/// Caution, Perceptive), its key ability's modifier; `None` for a skill
/// only the trained use that `worn` (the sheet without effects or items)
/// has no ranks in.
pub fn total(library: Option<&Library>, worn: &Sheet, now: &Sheet, skill: usize) -> Option<i16> {
    let base = *worn.skills.get(skill)?;
    if NEEDS_RANKS[skill] && base == 0 {
        return None;
    }
    let ability = library
        .and_then(|library| library.skill_ability(skill))
        .unwrap_or(KEY[skill]);
    Some((now.skill(skill) + now.modifier(ability)).clamp(-128, 127) as i16)
}

/// The rolls a creature holds for perception, drawn every
/// `ROLL_EVERY_MS`: as a sneaker its stealth roll against sight (`hide`)
/// and hearing (`hush`), `rolls::SNEAK_BASE` + a `rolls::SNEAK_DIE` each;
/// as an observer its spot and listen rolls, a `rolls::WATCH_DIE` each.
/// `drawn_at` 0: never drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rolls {
    pub hide: u8,
    pub hush: u8,
    pub spot: u8,
    pub listen: u8,
    pub drawn_at: u64,
}

impl Rolls {
    /// Each roll's middle: what a stage without scripts (no random
    /// stream kept for perception) uses.
    pub const MEAN: Rolls = Rolls {
        hide: 15,
        hush: 15,
        spot: 10,
        listen: 10,
        drawn_at: 1,
    };

    pub fn due(&self, now: u64) -> bool {
        self.drawn_at == 0 || now >= self.drawn_at + ROLL_EVERY_MS
    }

    pub fn draw(rng: &mut SimRng, now: u64) -> Rolls {
        Rolls {
            hide: rolls::SNEAK_BASE + 1 + rng.below(rolls::SNEAK_DIE) as u8,
            hush: rolls::SNEAK_BASE + 1 + rng.below(rolls::SNEAK_DIE) as u8,
            spot: 1 + rng.below(rolls::WATCH_DIE) as u8,
            listen: 1 + rng.below(rolls::WATCH_DIE) as u8,
            drawn_at: now.max(1),
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.bytes(&[self.hide, self.hush, self.spot, self.listen]);
        out.u64(self.drawn_at);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Rolls, DecodeError> {
        let [hide, hush, spot, listen] = input.array()?;
        Ok(Rolls {
            hide,
            hush,
            spot,
            listen,
            drawn_at: input.u64()?,
        })
    }
}

/// An observer as the checks read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Watcher {
    pub awareness: i16,
    pub rolls: Rolls,
    pub in_combat: bool,
    pub running: bool,
    pub still: bool,
    /// A creature (not a player's character).
    pub npc: bool,
    /// Its facing across the floor (unit).
    pub facing: [f32; 2],
    /// Its worn items give True Seeing: it sees the stealthed.
    pub true_seeing: bool,
}

/// A stealthed one as the checks read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sneaker {
    pub stealth: i16,
    pub rolls: Rolls,
    pub still: bool,
    /// A player's character.
    pub player: bool,
    /// `appearance.2da` `sizecategory` (1 tiny … 5 huge).
    pub size: u8,
    /// Its worn items dampen its sound (TSL's Dampen Sound): never heard.
    pub silent: bool,
}

/// The spot check's score (seen when above 0): Awareness and the spot
/// roll against Stealth and the stealth roll, less or more by the rules of
/// `rules::stealth::spot` (running, behind, in combat, standing still, the
/// distance); an observer with True Seeing always sees. `toward` is the sneaker less the
/// observer across the floor (metres).
pub fn spot_score(watcher: &Watcher, sneaker: &Sneaker, toward: [f32; 2]) -> i32 {
    if watcher.true_seeing {
        return i32::MAX;
    }
    let distance = (toward[0] * toward[0] + toward[1] * toward[1]).sqrt();
    let mut score = i32::from(watcher.awareness) + i32::from(watcher.rolls.spot)
        - i32::from(sneaker.stealth)
        - i32::from(sneaker.rolls.hide);
    if watcher.running {
        score -= spot::RUNNING;
    }
    let behind = distance > 1e-4
        && (watcher.facing[0] * toward[0] + watcher.facing[1] * toward[1]) / distance
            <= spot::BEHIND_COS;
    if watcher.npc && sneaker.player && behind {
        score -= spot::BEHIND;
    }
    if watcher.in_combat {
        score -= spot::IN_COMBAT;
    }
    if sneaker.still {
        score -= spot::STILL_SNEAKER;
    }
    if watcher.still {
        score += spot::STILL_WATCHER;
    }
    let thirds = (distance / spot::METRES_A_POINT) as i32;
    if thirds > spot::FREE_POINTS {
        score -= thirds - spot::FREE_POINTS;
    }
    score
}

/// What a creature's size (`sizecategory` 1 to 5) adds to being heard.
fn size_heard(size: u8) -> i32 {
    usize::from(size)
        .checked_sub(1)
        .and_then(|at| listen::SIZE.get(at))
        .copied()
        .unwrap_or(0)
}

/// The listen check's score (heard when above 0): Awareness and the listen
/// roll against Stealth and the stealth roll, a point off each
/// `listen::METRES_A_POINT`, the sneaker's size, the area's
/// `ModListenCheck`, `listen::WALL_A_METRE` off each metre of wall between
/// them; a silent sneaker is never heard.
pub fn listen_score(
    watcher: &Watcher,
    sneaker: &Sneaker,
    distance_m: f32,
    mod_listen: i8,
    wall_m: f32,
) -> i32 {
    if sneaker.silent {
        return i32::MIN;
    }
    i32::from(watcher.awareness) + i32::from(watcher.rolls.listen)
        - i32::from(sneaker.stealth)
        - i32::from(sneaker.rolls.hush)
        - (distance_m / listen::METRES_A_POINT) as i32
        + size_heard(sneaker.size)
        + i32::from(mod_listen)
        - (wall_m * listen::WALL_A_METRE) as i32
}
