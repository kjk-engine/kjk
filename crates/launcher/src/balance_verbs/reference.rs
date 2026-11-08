//! What KOTOR's own rules make of a matchup, from the sheets the sim fought
//! with: each side's chance to hit the other (`damage::chance`, its attack
//! bonus with its style's swing against the other's defence), the damage a
//! hit of its does (`weapon_roll`, the critical as `critical` rolls it, and
//! `weapon_damage` through the target's guards, averaged over many rolls of
//! the sim's own functions), and what a blow that lands in kjk is worth
//! (that hit times the chance, times the pace at the side's measured
//! cadence: the same terms `damage::assess` sizes a landed blow by, for a
//! blow to the torso with its whole share). From those, the attack rolls
//! KOTOR needs to kill the other (its HP over chance times damage) and the
//! landed blows kjk needs; KOTOR's winner is the side needing fewer rolls.

use serde::Serialize;
use sim::combat::Sheet;
use sim::combat::damage;
use sim::combat::passive::Game;
use sim::combat::saber::{Kit, Style};

/// Rolls averaged per side.
const ROLLS: u32 = 4000;

#[derive(Debug, Clone, Serialize)]
pub(super) struct Reference {
    /// The weapon swung, as the style swings it: dice, bonus, attack
    /// bonus, `baseitems.2da` row.
    pub weapon: String,
    /// The chance to hit (0 to 1).
    pub p: f64,
    /// A hit's damage, whole points (KOTOR's, criticals counted).
    pub hit: f64,
    /// A blow landed in kjk, whole points (`hit` × `p` × pace).
    pub landed: f64,
    pub cadence_ms: u32,
    /// The other's most HP, and what it takes to bring them down.
    pub foe_hp: f64,
    pub kotor_blows_to_kill: f64,
    pub kjk_landed_to_kill: f64,
}

/// A die for the sim's damage functions: splitmix64 from a fixed seed.
struct Dice(u64);

impl Dice {
    fn next(&mut self, sides: u8) -> i64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) % u64::from(sides.max(1))) as i64 + 1
    }
}

/// `attacker` swinging in `style` at `target`, its blows `cadence_ms`
/// apart (see the module documentation).
pub(super) fn reference(
    attacker: &Sheet,
    style: Style,
    target: &Sheet,
    cadence_ms: u32,
    game: Game,
) -> Reference {
    let kit = Kit::of(attacker);
    let weapon = kit.swing(style, attacker.main_weapon());
    let odds = damage::odds(attacker, &weapon, target);
    let p = damage::margin(odds.0, odds.1) as f64 / 1000.0;
    let confirm = damage::confirming(attacker, odds);
    let mut dice = Dice(0x5eed);
    let mut total = 0i64;
    for _ in 0..ROLLS {
        let roll = damage::weapon_roll(&weapon, Some(attacker), &mut |sides| dice.next(sides));
        let d20 = dice.next(20);
        let crit = damage::critical(d20, &weapon, target, Some(confirm), &mut || dice.next(20));
        let (hurt, _) = damage::weapon_damage(
            &weapon,
            Some(attacker),
            target,
            roll,
            crit,
            0,
            game,
            &mut |sides| dice.next(sides),
        );
        total += hurt;
    }
    let hit = total as f64 / f64::from(ROLLS);
    let cadence = cadence_ms.min(damage::WHOLE_CADENCE_MS);
    let pace = damage::pace(kit.paced(style, cadence)) as f64 / 1000.0;
    let landed = hit * p * pace;
    let foe_hp = f64::from(target.max_hp);
    let over = |per: f64| if per > 0.0 { foe_hp / per } else { f64::INFINITY };
    Reference {
        weapon: format!(
            "{}d{}{:+} ab{:+} row {} ({})",
            weapon.dice,
            weapon.die,
            weapon.damage_bonus,
            weapon.attack_bonus,
            weapon.base_item,
            style.name()
        ),
        p,
        hit,
        landed,
        cadence_ms: cadence,
        foe_hp,
        kotor_blows_to_kill: over(p * hit),
        kjk_landed_to_kill: over(landed),
    }
}
