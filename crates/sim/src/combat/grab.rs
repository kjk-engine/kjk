//! The grab with bare hands under the Advanced rules (`world::grapples`):
//! its roll, and the hold two characters are in while a grapple finisher
//! plays (`Grapple`, carried by each fighter).
//!
//! kjk has no roll to hit (a blow always lands, and the chance to hit
//! scales its damage), so the grab has a roll of its own, as KOTOR's
//! attack: a d20 plus the grabber's melee attack bonus against the victim's
//! Defense (`damage::odds`, which counts Strength once). A victim below
//! `HURT_BELOW_PCT` of its health counts its Defense `HURT_DEFENCE` lower,
//! any other `HEALTHY_DEFENCE` higher, so a grab is a finishing move. A
//! natural 20 always holds and a natural 1 always misses.

use rules::duel::grab;

use crate::combat::damage;
use crate::combat::sheet::{Sheet, Weapon};
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};

/// Whether a grab holds: `d20` (1 to 20) plus `attacker`'s unarmed attack
/// bonus against `target`'s Defense, changed by its health, `target_hp`
/// hundredths of an HP (see the module documentation).
pub fn grab_roll(d20: i64, attacker: &Sheet, target: &Sheet, target_hp: i32) -> bool {
    match d20 {
        20.. => return true,
        ..=1 => return false,
        _ => {}
    }
    let (attack, defence) = damage::odds(attacker, &Weapon::UNARMED, target);
    let defence = defence + defence_change(target, target_hp);
    d20 + i64::from(attack) >= i64::from(defence)
}

/// How much a grab counts `target`'s Defense lower (negative) or higher
/// at `hp` hundredths of an HP: the comparison `ai::choose_item` makes.
fn defence_change(target: &Sheet, hp: i32) -> i32 {
    let hurt =
        i64::from(hp) * 100 < i64::from(target.max_hp) * 100 * i64::from(grab::HURT_BELOW_PCT);
    if hurt {
        -grab::HURT_DEFENCE
    } else {
        grab::HEALTHY_DEFENCE
    }
}

/// One side of a grapple: who holds or is held, which finisher plays
/// (1 to 3), whether this is the grabber, when (sim ms) the finisher began,
/// how many of its blows have landed, whether a blow would have killed the
/// victim (its death waits for the finisher to let it go), and whether the
/// victim has been thrown and flies (the third finisher's end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grapple {
    pub partner: SimId,
    pub finisher: u8,
    pub grabber: bool,
    pub since: u64,
    pub blows: u8,
    pub dead: bool,
    pub flying: bool,
}

impl Grapple {
    /// The blank a fighter in no grapple writes in its place.
    pub(crate) const BLANK: Grapple = Grapple {
        partner: SimId(0),
        finisher: 0,
        grabber: false,
        since: 0,
        blows: 0,
        dead: false,
        flying: false,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.partner.0);
        out.u8(self.finisher);
        out.u8(u8::from(self.grabber) | u8::from(self.dead) << 1 | u8::from(self.flying) << 2);
        out.u64(self.since);
        out.u8(self.blows);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Grapple, DecodeError> {
        let partner = SimId(input.u32()?);
        let at = input.at();
        let finisher = input.u8()?;
        if finisher > 3 {
            return Err(input.invalid("grapple finisher", at));
        }
        let at = input.at();
        let flags = input.u8()?;
        if flags > 0b111 {
            return Err(input.invalid("grapple flags", at));
        }
        Ok(Grapple {
            partner,
            finisher,
            grabber: flags & 1 != 0,
            dead: flags & 2 != 0,
            flying: flags & 4 != 0,
            since: input.u64()?,
            blows: input.u8()?,
        })
    }
}

#[cfg(feature = "probes")]
pub fn grab_probes() -> crate::probe::Checks {
    use crate::combat::sheet::Ability;
    use crate::probe::check;

    /// A grabber with melee attack bonus `attack` and a target with
    /// Defense `defence` against it, at most 100 HP (`odds` as the roll reads
    /// it).
    fn pair(attack: i32, defence: i32) -> (Sheet, Sheet) {
        let mut grabber = Sheet::commoner();
        let mut target = Sheet::commoner();
        target.max_hp = 100;
        let (a, d) = damage::odds(&grabber, &Weapon::UNARMED, &target);
        grabber.bab = (i32::from(grabber.bab) + attack - a).clamp(0, 255) as u8;
        target.defence = (i32::from(target.defence) + defence - d).clamp(-128, 127) as i8;
        (grabber, target)
    }

    /// The least d20 that holds at `pct`% of the target's health.
    fn least(grabber: &Sheet, target: &Sheet, pct: i32) -> i64 {
        let hp = i32::from(target.max_hp) * pct;
        (1..=20)
            .find(|&d20| grab_roll(d20, grabber, target, hp))
            .unwrap_or(21)
    }

    let (grabber, target) = pair(5, 15);
    let healthy = least(&grabber, &target, 80);
    // Against Defense 25 a hurt target needs 25 − 10 − 5, a healthy one
    // 25 + 5 − 5: only a natural 20.
    let (_, guarded) = pair(5, 25);
    let hurt = least(&grabber, &guarded, 30);
    let at_line = least(&grabber, &guarded, grab::HURT_BELOW_PCT);
    // A 20 against a Defense far past reach, a 1 against none at all.
    let (strong, _) = pair(60, 0);
    let (_, far) = pair(0, 90);
    let twenty = grab_roll(20, &grabber, &far, 100 * 100);
    let one = grab_roll(1, &strong, &target, 0);
    // Strength 2 higher: its modifier, and the roll needed, 1 better.
    let mut stronger = grabber;
    let strength = Ability::Strength as usize;
    stronger.abilities[strength] = stronger.abilities[strength].saturating_add(2);
    let raised = least(&stronger, &target, 80);
    vec![
        (
            "hurt_target_easy",
            check(
                hurt == 10 && at_line == 20,
                format!(
                    "attack 5 against Defense 25: at 30% health a {hurt} holds (want 10); at \
                     {}% a {at_line} (want 20, as a healthy target)",
                    grab::HURT_BELOW_PCT
                ),
            ),
        ),
        (
            "healthy_target_hard",
            check(
                healthy == 15,
                format!("attack 5 against Defense 15 at 80% health: a {healthy} holds (want 15)"),
            ),
        ),
        (
            "natural_twenty_and_one",
            check(
                twenty && !one,
                format!("a 20 against Defense 90 holds: {twenty}; a 1 at attack 60 holds: {one}"),
            ),
        ),
        (
            "strength_counted_once",
            check(
                healthy - raised == 1,
                format!("Strength 2 higher: a {raised} holds, where a {healthy} did (want 1 less)"),
            ),
        ),
    ]
}
