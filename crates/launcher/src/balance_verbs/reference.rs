//! What KOTOR's own rules make of a matchup, from the sheets the sim fought
//! with: every attack KOTOR gives a side a round, and what the round is
//! worth against the other.
//!
//! A round's attacks (KOTOR has no iterative attacks from the base attack
//! bonus): the main hand's; a second one with two blades out (the dual
//! style, a weapon in each hand: the off hand's; the staff style, a
//! double-bladed weapon: as the main hand's, kjk's rule); one more of the
//! main hand's when the style carries Flurry (`Kit::swing`'s penalty on
//! every attack of the round); the extra attacks of Knight and Master Speed
//! (`effects::extra_attacks`, as many as ran on average over the fight) and
//! of TSL's Juyo (`forms::extra_attacks`). Each attack: its chance to hit
//! (`damage::odds` with the weapon as the hand holds it: two-weapon
//! fighting's penalty or Dueling's bonus, `passive::hand_attack`, and the
//! Form's, `forms::weapon`) and the damage a hit does (`weapon_roll`, the
//! critical as `critical` rolls it, `weapon_damage` through the target's
//! guards, averaged over many rolls of the sim's own functions).
//!
//! KOTOR's round is `damage::WHOLE_CADENCE_MS` of kjk's time (the pace,
//! `combat.damage.pace`, makes a blow at that cadence one attack's worth),
//! so a round's expected damage over that cadence, times the side's own
//! cadence, is what a kjk blow would carry to keep KOTOR's time to kill
//! (`per_blow`); `landed` is what kjk sizes one landed blow at (the main
//! hand's hit times its chance times the pace at the side's cadence, Speed's
//! and Juyo's extra attacks multiplying it, as `world::harm` does).

use serde::Serialize;
use sim::combat::Sheet;
use sim::combat::passive::{self, Game};
use sim::combat::saber::{Kit, Style};
use sim::combat::{Weapon, damage, forms};

/// Rolls averaged per attack.
const ROLLS: u32 = 4000;

/// One of KOTOR's attacks a round.
#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct Attack {
    /// The hand (0 right, 1 left) and how many of these the round has on
    /// average (Speed's run part of the fight).
    pub hand: u8,
    pub count: f64,
    /// The chance to hit (0 to 1) and a hit's damage (whole points,
    /// criticals counted).
    pub p: f64,
    pub hit: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Reference {
    /// The main hand's weapon, as the style swings it: dice, bonus, attack
    /// bonus, `baseitems.2da` row.
    pub weapon: String,
    /// The main hand's chance to hit and a hit's damage.
    pub p: f64,
    pub hit: f64,
    /// Every attack of a KOTOR round, and the round's expected damage.
    pub attacks: Vec<Attack>,
    pub round: f64,
    /// What a kjk blow at the side's cadence would carry for KOTOR's time
    /// to kill (the round over `WHOLE_CADENCE_MS`, times the cadence).
    pub per_blow: f64,
    /// A blow landed in kjk, whole points (`hit` × `p` × pace × extra).
    pub landed: f64,
    pub cadence_ms: u32,
    /// The other's most HP, and what it takes to bring them down.
    pub foe_hp: f64,
    pub kotor_rounds_to_kill: f64,
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

/// `attacker`'s weapon in `hand` as the hand holds it (`world::firing`'s
/// `in_hand`, without a battle stim) and `style` swings it.
fn in_hand(attacker: &Sheet, kit: &Kit, style: Style, hand: u8) -> Weapon {
    let mut weapon = forms::weapon(attacker, attacker.held(hand));
    let aim = passive::hand_attack(attacker, hand);
    weapon.attack_bonus = (i32::from(weapon.attack_bonus) + aim).clamp(-128, 127) as i8;
    kit.swing(style, weapon)
}

/// `weapon`'s chance to hit `target` and its mean damage on a hit.
fn attack(attacker: &Sheet, weapon: &Weapon, target: &Sheet, game: Game) -> (f64, f64) {
    let odds = damage::odds(attacker, weapon, target);
    let p = damage::margin(odds.0, odds.1) as f64 / 1000.0;
    let confirm = damage::confirming(attacker, odds);
    let mut dice = Dice(0x5eed);
    let mut total = 0i64;
    for _ in 0..ROLLS {
        let roll = damage::weapon_roll(weapon, Some(attacker), &mut |sides| dice.next(sides));
        let d20 = dice.next(20);
        let crit = damage::critical(d20, weapon, target, Some(confirm), &mut || dice.next(20));
        let (hurt, _) = damage::weapon_damage(
            weapon,
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
    (p, total as f64 / f64::from(ROLLS))
}

/// `attacker` fighting in `style` at `target`, its blows `cadence_ms`
/// apart, `speed` Speed's extra attacks a round on average (see the module
/// documentation).
pub(super) fn reference(
    attacker: &Sheet,
    style: Style,
    target: &Sheet,
    cadence_ms: u32,
    speed: f64,
    game: Game,
) -> Reference {
    let kit = Kit::of(attacker);
    let main = in_hand(attacker, &kit, style, 0);
    let (p, hit) = attack(attacker, &main, target, game);
    // The main hand's extra attacks: Flurry's, Speed's, Juyo's.
    let flurry = kit.paced(style, 1000) > 1000;
    let extra = speed + f64::from(forms::extra_attacks(attacker));
    let mut attacks = vec![Attack {
        hand: 0,
        count: 1.0 + f64::from(u8::from(flurry)) + extra,
        p,
        hit,
    }];
    // The second blade's: the dual style's left hand, the staff's other end
    // (the main hand's, kjk's rule for a double-bladed weapon).
    let second = match style {
        Style::Dual => Some(1),
        Style::Staff => Some(0),
        _ => None,
    };
    if let Some(hand) = second {
        let (p, hit) = if hand == 0 {
            (p, hit)
        } else {
            attack(attacker, &in_hand(attacker, &kit, style, hand), target, game)
        };
        attacks.push(Attack {
            hand: 1,
            count: 1.0,
            p,
            hit,
        });
    }
    let round: f64 = attacks.iter().map(|a| a.count * a.p * a.hit).sum();
    let cadence = cadence_ms.min(damage::WHOLE_CADENCE_MS);
    let per_blow = round * f64::from(cadence) / f64::from(damage::WHOLE_CADENCE_MS);
    let pace = damage::pace(kit.paced(style, cadence)) as f64 / 1000.0;
    let landed = hit * p * pace * (1.0 + extra);
    let foe_hp = f64::from(target.max_hp);
    let over = |per: f64| if per > 0.0 { foe_hp / per } else { f64::INFINITY };
    Reference {
        weapon: format!(
            "{}d{}{:+} ab{:+} row {} ({})",
            main.dice,
            main.die,
            main.damage_bonus,
            main.attack_bonus,
            main.base_item,
            style.name()
        ),
        p,
        hit,
        attacks,
        round,
        per_blow,
        landed,
        cadence_ms: cadence,
        foe_hp,
        kotor_rounds_to_kill: over(round),
        kjk_landed_to_kill: over(landed),
    }
}
