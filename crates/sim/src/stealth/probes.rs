//! The probes of `stealth` (`kjk sim probes`; the approved test
//! `stealth_spot`).

use super::{Rolls, Sneaker, Watcher, listen_score, spot_score};

fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

const ROLLS: Rolls = Rolls {
    hide: 15,
    hush: 15,
    spot: 10,
    listen: 10,
    drawn_at: 1,
};

fn watcher() -> Watcher {
    Watcher {
        awareness: 6,
        rolls: ROLLS,
        in_combat: false,
        running: false,
        still: false,
        npc: true,
        facing: [1.0, 0.0],
        true_seeing: false,
    }
}

fn sneaker() -> Sneaker {
    Sneaker {
        stealth: 4,
        rolls: ROLLS,
        still: false,
        player: true,
        size: 3,
        silent: false,
    }
}

/// The spot and listen scores at fixed rolls (the spec's S12).
fn scores() -> Vec<(&'static str, Result<String, String>)> {
    let (w, s) = (watcher(), sneaker());
    // 6 + 10 - 4 - 15 = -3 in front at 3 m (no distance term until 9 m).
    let ahead = spot_score(&w, &s, [3.0, 0.0]);
    // Behind an NPC: 5 more off.
    let behind = spot_score(&w, &s, [-3.0, 0.0]);
    // 15 m: floor(15 / 3) - 2 = 3 off.
    let far = spot_score(&w, &s, [15.0, 0.0]);
    // The observer still: +5; the sneaker still too: -5.
    let still = Watcher { still: true, ..w };
    let observer_still = spot_score(&still, &s, [3.0, 0.0]);
    let both_still = spot_score(&still, &Sneaker { still: true, ..s }, [3.0, 0.0]);
    // In combat: 10 off; running: 5 off.
    let busy = Watcher {
        in_combat: true,
        running: true,
        ..w
    };
    let busy = spot_score(&busy, &s, [3.0, 0.0]);
    // A player watching a creature: no behind term.
    let peer = spot_score(
        &Watcher { npc: false, ..w },
        &Sneaker { player: false, ..s },
        [-3.0, 0.0],
    );
    // Listen: 6 + 10 - 4 - 15 - floor(6 / 3) = -5; a huge one (size 5)
    // +8; a metre of wall -5.
    let listen = listen_score(&w, &s, 6.0, 0, 0.0);
    let huge = listen_score(&w, &Sneaker { size: 5, ..s }, 6.0, 0, 0.0);
    let walled = listen_score(&w, &s, 6.0, 0, 1.0);
    // Worn items: True Seeing sees through any stealth; a dampened
    // sneaker is never heard.
    let seeing = spot_score(
        &Watcher {
            true_seeing: true,
            ..w
        },
        &s,
        [30.0, 0.0],
    );
    let dampened = listen_score(
        &w,
        &Sneaker {
            silent: true,
            size: 5,
            ..s
        },
        1.0,
        0,
        0.0,
    );
    vec![
        ("spot_ahead", check(ahead == -3, ahead.to_string())),
        ("spot_behind", check(behind == -8, behind.to_string())),
        ("spot_far", check(far == -6, far.to_string())),
        (
            "spot_observer_still",
            check(observer_still == 2, observer_still.to_string()),
        ),
        (
            "spot_both_still",
            check(both_still == -3, both_still.to_string()),
        ),
        ("spot_busy", check(busy == -18, busy.to_string())),
        (
            "spot_no_behind_for_players",
            check(peer == -3, peer.to_string()),
        ),
        ("listen", check(listen == -5, listen.to_string())),
        ("listen_size", check(huge == 3, huge.to_string())),
        ("listen_wall", check(walled == -10, walled.to_string())),
        ("spot_true_seeing", check(seeing > 0, seeing.to_string())),
        (
            "listen_dampened",
            check(dampened <= 0, dampened.to_string()),
        ),
    ]
}

/// A skill's total: ranks with effects and items, the key ability, Gear
/// Head; none for Stealth, Demolitions or Security without base ranks.
/// The rolls drawn within their bounds and held 20 s.
fn totals() -> Vec<(&'static str, Result<String, String>)> {
    use crate::combat::Sheet;
    use crate::combat::sheet::skills;
    let mut worn = Sheet::dev_soldier();
    worn.skills[skills::STEALTH] = 0;
    worn.skills[skills::AWARENESS] = 0;
    let mut now = worn;
    // An item's +6 on no ranks; Awareness 2 from an effect.
    now.skills[skills::STEALTH] = 6;
    now.skills[skills::AWARENESS] = 2;
    // DEX 14 (+2) keys Stealth and WIS 12 (+1) Awareness without a
    // library (K1's `skills.2da`).
    let untrained = super::total(None, &worn, &now, skills::STEALTH);
    let aware = super::total(None, &worn, &now, skills::AWARENESS);
    worn.skills[skills::STEALTH] = 1;
    let trained = super::total(None, &worn, &now, skills::STEALTH);
    let mut rng = crate::rng::SimRng::new(7, 3);
    let rolls = Rolls::draw(&mut rng, 5000);
    let in_bounds = (11..=20).contains(&rolls.hide)
        && (11..=20).contains(&rolls.hush)
        && (1..=20).contains(&rolls.spot)
        && (1..=20).contains(&rolls.listen);
    vec![
        (
            "total_needs_ranks",
            check(untrained.is_none(), format!("{untrained:?}")),
        ),
        (
            "total_untrained_awareness",
            check(aware == Some(3), format!("{aware:?}")),
        ),
        (
            "total_trained",
            check(trained == Some(8), format!("{trained:?}")),
        ),
        ("rolls_bounds", check(in_bounds, format!("{rolls:?}"))),
        (
            "rolls_due",
            check(!rolls.due(24_999) && rolls.due(25_000), "20 s".into()),
        ),
    ]
}

/// Worn items' skill bonuses (S3): the Eriadu unit (`g_i_belt008`:
/// property 36, Stealth, +6; base item 88) in the belt slot adds 6 to
/// Stealth as effects do, marks the sheet as wearing a stealth unit, and
/// gives no ranks of its own (the sheet without effects keeps 0); taken
/// off, both go.
fn worn_items() -> Vec<(&'static str, Result<String, String>)> {
    use crate::combat::sheet::{flags, skills};
    use crate::inventory::{ItemFacts, Property, slots};
    let unit = ItemFacts {
        base_item: 88,
        slots: 1 << slots::BELT,
        properties: vec![Property::Skill {
            skill: skills::STEALTH as u8,
            by: 6,
        }],
        stealth_unit: true,
        shield: false,
        ..crate::world::containers::plain("g_i_belt008")
    }
    .made();
    let worn = crate::inventory::dress(crate::combat::Sheet::dev_soldier(), [(slots::BELT, &unit)]);
    let now = crate::combat::effects::sheet_now(worn, &crate::combat::Fighter::of(&worn), 0);
    let off = crate::inventory::rewear(worn, &[(slots::BELT, &unit)], &[]);
    vec![
        (
            "item_skill_folds",
            check(
                now.skills[skills::STEALTH] == 6,
                format!("{:?}", now.skills),
            ),
        ),
        (
            "item_skill_not_ranks",
            check(
                worn.skills[skills::STEALTH] == 0,
                format!("{:?}", worn.skills),
            ),
        ),
        (
            "stealth_unit_flag",
            check(
                worn.flags & flags::STEALTH_UNIT != 0,
                format!("{:#x}", worn.flags),
            ),
        ),
        (
            "stealth_unit_off",
            check(
                off.flags & flags::STEALTH_UNIT == 0 && off.gear.skills == [0; 8],
                format!("{:#x} {:?}", off.flags, off.gear.skills),
            ),
        ),
    ]
}

/// Every check (see the module documentation).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let mut all = scores();
    all.extend(totals());
    all.extend(worn_items());
    all.extend(crate::world::covert::probes::stealth_mode());
    all.extend(crate::world::covert::probes::perception());
    all.extend(crate::world::covert::probes::opener());
    all.extend(crate::world::covert::probes::stealth_xp_loss());
    all
}
