//! A pistol in each hand: KOTOR's two pistols (as reone has them, the
//! `b6a`/`g6r1` clips), whose round is an attack for each hand. Here the
//! guns take turns on the trigger, right then left, each on its own weapon
//! timer at its own refire, and neither fires within the pair's share of a
//! refire after the other (`GAP`): held down, a pair fires twice as often as one gun, each shot the
//! hand's own (its weapon, its two-weapon penalty: `passive::hand_attack`;
//! its muzzle: `world::hand_muzzle`).
//!
//! The feat specials (`combat::specials`) are the main gun's, as KOTOR's
//! feat attack is the main hand's and the off hand keeps its one attack:
//! - **Rapid Shot**'s burst is the right gun's, the left firing its one
//!   shot the pair's share of a burst gap after the first (main, off, main; Multi Shot
//!   main, off, main, main);
//! - **Power Blast**'s charge goes off from both guns at once, for one
//!   cost;
//! - **Sniper Shot** aims the right gun alone; the left's timer runs on.

use crate::combat::specials::{self, BURST_GAP_MS, Shooting, Shot, Special, Trigger};
use crate::combat::weapons;

/// The share (thousandths) of a gun's refire the other gun of a pair waits
/// after it fires, and of a burst's gap the left's Rapid Shot shot leaves
/// after the first (`weapons.pistol.pair_gap`).
const GAP: u32 = rules::weapons::pistol::PAIR_GAP;

/// `ms` at the pair's gap share.
fn gap(ms: u32) -> i32 {
    (ms * GAP / 1000) as i32
}

/// Each hand's gun (right, left): its refire (ms, Rage's factor applied)
/// and the attack's time its shot's damage counts (C2, ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hands {
    pub refires: [u32; 2],
    pub paces: [u32; 2],
}

/// One step of a pair's trigger, without a special: `main_ms` is the right
/// gun's timer (`Fighter::weapon_wait_ms`), the left's is `shooting.off_ms`. The
/// hand that fires, if one does.
pub fn pull(
    shooting: &mut Shooting,
    main_ms: &mut i32,
    trigger: Trigger,
    refires: [u32; 2],
) -> Option<u8> {
    shooting.alt_held = trigger.alt;
    let mut timers = [*main_ms, shooting.off_ms];
    let ready = timers.map(|mut timer| {
        let ready = weapons::count_down(&mut timer, trigger.msec);
        (timer, ready)
    });
    timers = ready.map(|(timer, _)| timer);
    let fired = if trigger.attack || trigger.alt {
        let hand = usize::from(shooting.next_off);
        ready[hand].1.then(|| {
            timers[hand] += refires[hand] as i32;
            let other = 1 - hand;
            timers[other] = timers[other].max(gap(refires[other]));
            hand
        })
    } else {
        // Let go: a gun ready waits from now (`weapons::trigger`), so no
        // shot is banked.
        for (timer, (_, ready)) in timers.iter_mut().zip(ready) {
            if ready {
                *timer = 0;
            }
        }
        None
    };
    [*main_ms, shooting.off_ms] = timers;
    let hand = fired?;
    shooting.next_off = hand == 0;
    shooting.hands = 1 << hand;
    Some(hand as u8)
}

/// One step of a pair's trigger with `special` on the alternate button
/// (see the module documentation; `pool` and `cost` as `specials::pull`
/// has them): the shot each hand fires (right, left).
pub fn pull_special(
    shooting: &mut Shooting,
    (main_ms, pool): (&mut i32, &mut i32),
    (special, cost): (Option<Special>, i32),
    trigger: Trigger,
    hands: Hands,
) -> [Option<Shot>; 2] {
    let press = trigger.alt && !shooting.alt_held;
    let affordable = *pool >= cost;
    let special_on = match special {
        Some(Special::PowerBlast(_)) => {
            shooting.charging_from.is_some() || (trigger.alt && affordable)
        }
        Some(Special::SniperShot(_)) => trigger.alt,
        Some(Special::RapidShot(_)) => {
            shooting.burst > 0 || shooting.off_burst || (press && affordable)
        }
        None => false,
    };
    let mut shots = [None; 2];
    if special_on {
        let bursting = shooting.burst > 0;
        weapons::count_down(&mut shooting.off_ms, trigger.msec);
        let main = specials::pull(shooting, main_ms, pool, special, cost, trigger);
        shots[0] = main;
        match (special, main) {
            (Some(Special::PowerBlast(_)), Some(shot)) if shot.special.is_some() => {
                let charge = shot.cadence_ms.saturating_sub(hands.paces[0]);
                shots[1] = Some(Shot {
                    cadence_ms: hands.paces[1] + charge,
                    ..shot
                });
                shooting.off_ms = shooting.off_ms.max(0) + hands.refires[1] as i32;
            }
            // A burst begun (its first shot fired or waiting on the gun).
            // The left fires the pair's share of a gap after the right's first shot,
            // fired now or once the right is ready (raised, say).
            (Some(Special::RapidShot(_)), _) if !bursting && shooting.burst > 0 => {
                shooting.off_burst = true;
                let after = gap(BURST_GAP_MS.min(hands.refires[0]));
                let first = if main.is_some() { 0 } else { (*main_ms).max(0) };
                shooting.off_ms = shooting.off_ms.max(first + after);
            }
            _ => {}
        }
        if shooting.off_burst && shots[0].is_none() && shooting.off_ms <= 0 {
            shooting.off_burst = false;
            shooting.off_ms += hands.refires[1] as i32;
            shots[1] = Some(Shot {
                special,
                cadence_ms: hands.paces[1],
            });
        }
    } else {
        // Without a special, or the bar for one, the alternate fires as the
        // primary does (`specials::pull`).
        shooting.charging_from = None;
        shooting.burst = 0;
        shooting.off_burst = false;
        shooting.aiming = false;
        let plain = Trigger {
            alt: trigger.alt && (special.is_none() || !affordable),
            ..trigger
        };
        if let Some(hand) = pull(shooting, main_ms, plain, hands.refires) {
            shots[usize::from(hand)] = Some(Shot {
                special: None,
                cadence_ms: hands.paces[usize::from(hand)],
            });
        }
        shooting.alt_held = trigger.alt;
    }
    let fired = shots
        .iter()
        .enumerate()
        .filter(|(_, shot)| shot.is_some())
        .fold(0, |bits, (hand, _)| bits | 1 << hand);
    if fired != 0 {
        shooting.hands = fired;
    }
    shots
}
