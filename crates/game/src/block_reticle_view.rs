//! The arcs of targeted blocks around the centre of the screen
//! (`ui::screens::hud::block_arcs`), from the presented snapshot. They show
//! while the player holds the block button and the body the player controls
//! holds a lightsaber or a melee weapon, and only when the snapshot says
//! targeted blocks apply. They stay while the body cannot guard for a
//! moment, in a parry, a bounce or a stagger, as long as the button is
//! down. The
//! body the player controls is the companion the player leads, if any, and
//! otherwise the player's own character (`Snapshot::body`). The local
//! setting is never read: a guest plays by the host's choice.
//!
//! The aim arc sits at the direction of the body's last camera turn, which
//! the snapshot keeps (`sim::combat::saber::duel::aim::Turn`). The swing
//! arc shows the swing of the threat, the hostile fighter whose blow will
//! arrive first (`aim::threat`). The view reads both sides from
//! `aim::sides`, as the block rule does, so the colours and the rule always
//! agree.
//!
//! The perfect block mark is on for 600 ms after the body the player
//! controls makes a perfect block under the Advanced rules (`perfect_mark`).
//! While the arcs show, they show the mark. Otherwise a ring around the
//! centre of the screen shows it (`ui_view`): with targeted blocks off,
//! with the block button released, or under a camera that shows no crosshair.

use std::collections::HashMap;

use bevy::prelude::*;
use sim::combat::saber::duel::{self, aim};
use sim::{EntityState, Instances, Snapshot, math};
use ui::vm::{BlockReticleView, Side, Swing};

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

pub struct BlockReticlePlugin;

impl Plugin for BlockReticlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            show.run_if(resource_exists::<PresentedSnapshot>)
                .before(crate::ui_view::Stepped),
        );
    }
}

/// The arcs shown, from the body the presented snapshot's seat controls
/// and the fighters hostile to it. Nothing is looked up in a session that
/// does not play targeted blocks, or while that body is down or the
/// player does not hold the block button.
fn show(
    presented: Res<PresentedSnapshot>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    mut ui: ResMut<Ui>,
) {
    let snapshot = presented.0.current();
    let seat = seat.map_or(sim::Seat(0), |seat| seat.0);
    let shown: Option<BlockReticleView> = swap
        .as_ref()
        .filter(|_| snapshot.targeted())
        .zip(snapshot.body(seat).filter(|own| button_down(own)))
        .and_then(|(swap, own)| {
            let objects = Instances::in_snapshot(&swap.level, snapshot);
            let standing = swap.level.module().map(|module| snapshot.standings(module));
            let hostile = crate::battle_music::hostility(standing);
            let (bodies, _) = crate::battle_music::bodies_and_blows(snapshot, &objects);
            let by_id: HashMap<u32, _> = bodies.iter().map(|body| (body.id, body)).collect();
            let mine = by_id.get(&own.id.0)?;
            let foes: Vec<&EntityState> = snapshot
                .entities
                .iter()
                .filter(|entity| {
                    by_id
                        .get(&entity.id.0)
                        .is_some_and(|it| it.alive && hostile(it, mine))
                })
                .collect();
            of(snapshot, own, &objects, &foes)
        });
    ui.vm.perfect_mark = snapshot
        .body(seat)
        .is_some_and(|own| perfect_mark(snapshot, own));
    ui.vm.block_reticle = shown;
}

/// How long the perfect block mark stays on after a perfect block (ms).
/// The sim keeps no mark that ends when the blocker lets go of block,
/// bounces or attacks, so the view shows the mark for this long instead.
pub const PERFECT_MARK_MS: u64 = 600;

/// Whether the perfect block mark is on for `own` in `snapshot`. It is on
/// under the Advanced rules for 600 ms from `own`'s last perfect block
/// (`Duel::perfect_at`), while `own` is alive.
pub fn perfect_mark(snapshot: &Snapshot, own: &EntityState) -> bool {
    own.fighter.is_some_and(|fighter| {
        let at = fighter.saber.duel.perfect_at;
        snapshot.saber_rules.duel()
            && fighter.alive()
            && at != 0
            && (at..at + PERFECT_MARK_MS).contains(&snapshot.time_ms)
    })
}

/// The last perfect block of the body that `seat` controls in `snapshot`
/// (`Duel::perfect_at`), 0 for none. The probes of the perfect block's
/// mark and of its sound, flare and shake read it.
#[cfg(feature = "probes")]
pub(crate) fn perfect_at(snapshot: &Snapshot, seat: sim::Seat) -> u64 {
    (snapshot.body(seat))
        .and_then(|own| own.fighter)
        .map_or(0, |fighter| fighter.saber.duel.perfect_at)
}

/// Whether `own` is alive and the player holds the block button
/// (`aim::Turn::held`). The body need not be able to guard.
fn button_down(own: &EntityState) -> bool {
    own.fighter
        .is_some_and(|fighter| fighter.alive() && fighter.saber.turn.held)
}

/// The side as the HUD names it.
fn side(side: aim::Side) -> Side {
    match side {
        aim::Side::Left => Side::Left,
        aim::Side::Right => Side::Right,
        aim::Side::Above => Side::Above,
    }
}

/// What the HUD draws around the centre of the screen for `own`, the body
/// the player controls in `snapshot` (the companion the player leads, if
/// any, else the player's own character), with `objects` holding the
/// snapshot's sheets, against the hostile fighters `foes`. None while
/// targeted blocks do not apply, and while `own` is down or holds no blade,
/// or the player does not hold the block button. The aim comes from `own`'s last camera turn,
/// whether or not there is a threat. The arcs are marked while the perfect
/// block mark is on (`perfect_mark`).
pub fn of(
    snapshot: &Snapshot,
    own: &EntityState,
    objects: &Instances,
    foes: &[&EntityState],
) -> Option<BlockReticleView> {
    let (ps, turn) = (own.player?, own.fighter?.saber.turn);
    if !(snapshot.targeted() && button_down(own) && crate::duel_view::blade_user(own, objects)) {
        return None;
    }
    let unthreatened = BlockReticleView {
        aim: turn.aim,
        turned: turn.side().map(side),
        swing: None,
        marked: perfect_mark(snapshot, own),
    };
    let foes: Vec<_> = foes
        .iter()
        .filter_map(|foe| Some((foe.player?, foe.fighter?.saber)))
        .collect();
    let near: Vec<aim::Near> = foes
        .iter()
        .map(|(foe, saber)| aim::Near {
            distance: math::length(math::sub(foe.origin, ps.origin)),
            in_front: duel::in_front(ps.origin, ps.view[1], foe.origin),
            swing_began: aim::swinging(saber.current_move).then_some(saber.duel.swing_began),
        })
        .collect();
    let Some(threat) = aim::threat(&near) else {
        return Some(unthreatened);
    };
    let (foe, saber) = &foes[threat];
    let (swing, turned) = aim::sides(&ps, &turn, foe, saber.current_move);
    Some(BlockReticleView {
        turned: turned.map(side),
        swing: near[threat]
            .swing_began
            .map(|_| swing.map_or(Swing::Any, |s| Swing::Side(side(s)))),
        ..unthreatened
    })
}

/// The probes (`kjk sim probes --only "block reticle view"`): when the view
/// shows, that it follows the snapshot's flag and not the local setting,
/// and the sides it shows in a staged fight.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::trace::duelling_probes::targeted::{Stage, staged};
    let views_of = |stage| -> Staged {
        let views = staged(stage)?.into_iter().filter_map(|snapshot| {
            let mv = snapshot.body(sim::Seat(0))?.fighter?.saber.current_move;
            let view = seat_view(&snapshot);
            Some((snapshot, view, mv))
        });
        Ok(views.collect())
    };
    let left = views_of(Stage::TurnedLeft);
    let unturned = views_of(Stage::Unturned);
    let spin = views_of(Stage::Spin);
    let shown = left.as_ref().map_err(Clone::clone).and_then(|left| {
        let (snapshot, ..) = left
            .iter()
            .find(|(_, view, _)| view.is_some_and(|view| view.swing.is_some()))
            .ok_or("no tick shows a swing")?;
        Ok(snapshot.clone())
    });
    vec![
        ("view_shown", shown.clone().and_then(|s| view_shown(&s))),
        (
            "guest_reads_snapshot",
            shown.and_then(|s| guest_reads_snapshot(&s)),
        ),
        (
            "perfect_mark",
            perfect_mark_shown(left.clone(), unturned.clone()),
        ),
        ("arcs_through_parry", arcs_through_parry(left.clone())),
        ("view_sides", view_sides(left, unturned, spin)),
        ("dot_hidden_while_blocking", dot_hidden_while_blocking()),
    ]
}

/// The arcs stay while the block button stays down. In the staged fight
/// where seat 1 turned its camera to the left, seat 1 keeps the block
/// button down and makes a perfect block. Its perfect parry leaves it
/// unable to guard for some steps (`Duel::block_held` false). Every view in
/// the 600 ms after the perfect block still shows the arcs, marked, with
/// the aim to the left.
#[cfg(feature = "probes")]
fn arcs_through_parry(left: Staged) -> Result<String, String> {
    let left = left?;
    let at = (left.iter())
        .map(|(s, ..)| perfect_at(s, sim::Seat(1)))
        .find(|&at| at != 0)
        .ok_or("seat 1 makes no perfect block")?;
    let after: Vec<&(Snapshot, Option<BlockReticleView>, _)> = (left.iter())
        .filter(|(s, ..)| (at..at + PERFECT_MARK_MS).contains(&s.time_ms))
        .collect();
    let unguarded = (after.iter())
        .filter(|(s, ..)| {
            let own = s.body(sim::Seat(1)).and_then(|own| own.fighter);
            own.is_some_and(|fighter| !fighter.saber.duel.block_held)
        })
        .count();
    let wrong: Vec<String> = (after.iter())
        .filter(|(_, view, _)| {
            !view.is_some_and(|view| view.marked && view.turned == Some(Side::Left))
        })
        .map(|(s, view, _)| format!("{} ms: {view:?}", s.time_ms))
        .collect();
    match (unguarded > 0 && !after.is_empty(), wrong.is_empty()) {
        (true, true) => Ok(format!(
            "the perfect block at {at} ms; all {} views of the 600 ms after it show the marked arcs with the aim to the left, {unguarded} of them while seat 1 cannot guard",
            after.len()
        )),
        _ => Err(format!(
            "the perfect block at {at} ms; {} views after it, {unguarded} while seat 1 cannot guard; these without the marked arcs aimed left: {}",
            after.len(),
            wrong.join("; ")
        )),
    }
}

/// The perfect block mark (`perfect_mark`) that a seat's player sees: the
/// arcs marked, or, with the mark on and no arcs, the ring. In the staged
/// fight where seat 1 turned its camera to the left, seat 1 makes a perfect
/// block. From that snapshot seat 1's arcs are marked until 600 ms have
/// passed. Letting go of the block button within that time does not end the mark, and
/// the ring shows it in place of the arcs, as it does with the flag off.
/// Seat 0, the attacker, sees no mark. A dead body has no mark. Under Jedi
/// Academy's rules nothing shows. A guest's decoded snapshot shows the same
/// as the host's. In the fight where seat 1 has not turned its camera, the
/// block is ordinary, and no snapshot shows the mark.
#[cfg(feature = "probes")]
fn perfect_mark_shown(left: Staged, unturned: Staged) -> Result<String, String> {
    use sim::Seat;
    use sim::combat::saber::SaberRules;
    /// Whether the arcs are marked (None: no arcs), and whether the ring
    /// shows.
    type Seen = (Option<bool>, bool);
    let seen = |s: &Snapshot, seat: Seat| -> Seen {
        let objects = Instances::in_snapshot(&sim::Level::arena(), s);
        let (own, foe) = (s.body(seat), s.body(Seat(1 - seat.0)));
        let view = own
            .zip(foe)
            .and_then(|(own, foe)| of(s, own, &objects, &[foe]));
        let mark = own.is_some_and(|own| perfect_mark(s, own));
        (view.map(|view| view.marked), mark && view.is_none())
    };
    let (blocked, at) = left?
        .into_iter()
        .find_map(|(s, ..)| {
            Some((
                s.clone(),
                Some(perfect_at(&s, Seat(1))).filter(|&at| at != 0)?,
            ))
        })
        .ok_or("seat 1 makes no perfect block")?;
    let later = |ms: u64| move |s: &mut Snapshot| s.time_ms = at + ms;
    let released = |s: &mut Snapshot| {
        later(100)(s);
        if let Some(fighter) = blocker(s).fighter.as_mut() {
            fighter.saber.duel.block_held = false;
            fighter.saber.turn = sim::combat::saber::duel::aim::Turn::KEYED;
        }
    };
    let dead = |s: &mut Snapshot| {
        if let Some(fighter) = blocker(s).fighter.as_mut() {
            fighter.life = sim::combat::Life::Dead;
        }
    };
    type Change<'a> = &'a dyn Fn(&mut Snapshot);
    let (blocker_seat, attacker_seat) = (Seat(1), Seat(0));
    let rows: [(&str, Seat, Change, Seen); 8] = [
        (
            "the perfect block",
            blocker_seat,
            &|_| {},
            (Some(true), false),
        ),
        (
            "599 ms after it",
            blocker_seat,
            &later(599),
            (Some(true), false),
        ),
        (
            "600 ms after it",
            blocker_seat,
            &later(600),
            (Some(false), false),
        ),
        (
            "block released 100 ms after it",
            blocker_seat,
            &released,
            (None, true),
        ),
        (
            "the flag off",
            blocker_seat,
            &|s| s.targeted_blocks = false,
            (None, true),
        ),
        ("the blocker dead", blocker_seat, &dead, (None, false)),
        ("the attacker's view", attacker_seat, &|_| {}, (None, false)),
        (
            "Jedi Academy's rules",
            blocker_seat,
            &|s| s.saber_rules = SaberRules::Ja,
            (None, false),
        ),
    ];
    let guest = Snapshot::decode(&blocked.encode()).map_err(|e| format!("{e:?}"))?;
    let got: Vec<(String, Seen, Seen)> = (rows.iter())
        .map(|(name, seat, change, want)| {
            let mut s = blocked.clone();
            change(&mut s);
            (name.to_string(), seen(&s, *seat), *want)
        })
        .chain([(
            "the guest's snapshot".into(),
            seen(&guest, blocker_seat),
            (Some(true), false),
        )])
        .collect();
    let ordinary: Vec<Seen> = (unturned?.iter())
        .map(|(s, ..)| seen(s, blocker_seat))
        .filter(|&(marked, ring)| marked == Some(true) || ring)
        .collect();
    let wrong: Vec<String> = (got.iter())
        .filter(|(_, got, want)| got != want)
        .map(|(name, got, want)| format!("{name}: {got:?}, expected {want:?}"))
        .chain(
            (!ordinary.is_empty())
                .then(|| format!("the ordinary block's fight shows the mark: {ordinary:?}")),
        )
        .collect();
    match wrong.is_empty() {
        true => Ok(format!(
            "the perfect block at {at} ms, {} rows right, and no mark in the ordinary block's fight",
            got.len()
        )),
        false => Err(wrong.join("; ")),
    }
}

/// The HUD's crosshair dot (`ui_view::dot_shown`). With no arcs it shows as
/// it did before the arcs existed: while the crosshair is up and no
/// blaster's crosshair takes its place. With the arcs it never shows. The
/// HUD's redraw key holds the same value as `dot_shown`, so the key changes
/// too when blocking starts and stops.
#[cfg(feature = "probes")]
fn dot_hidden_while_blocking() -> Result<String, String> {
    use ui::vm::{ReticleView, ViewModel};
    let blaster = ReticleView {
        family: ui::screens::hud::Family::Cross,
        cone: 1.0,
        fov: 70.0,
    };
    let mut rows = Vec::new();
    for crosshair in [false, true] {
        for reticle in [None, Some(blaster)] {
            for arcs in [None, Some(BlockReticleView::default())] {
                let vm = ViewModel {
                    crosshair,
                    reticle,
                    block_reticle: arcs,
                    ..ViewModel::default()
                };
                let before_arcs = crosshair && reticle.is_none();
                let want = arcs.is_none() && before_arcs;
                let got = crate::ui_view::dot_shown(&vm);
                rows.push((crosshair, reticle.is_some(), arcs.is_some(), got, want));
            }
        }
    }
    let wrong: Vec<String> = (rows.iter())
        .filter(|(.., got, want)| got != want)
        .map(|(crosshair, blaster, arcs, got, _)| {
            format!("crosshair {crosshair}, a blaster {blaster}, the arcs {arcs}: shown {got}")
        })
        .collect();
    match wrong.is_empty() {
        true => Ok(format!(
            "{} rows right; with a saber and the crosshair up, the dot shows until blocking starts and shows again when it stops",
            rows.len()
        )),
        false => Err(wrong.join("; ")),
    }
}

/// The views of seat 1's character against seat 0's in a staged fight, each
/// with its snapshot and seat 0's move.
#[cfg(feature = "probes")]
type Staged = Result<Vec<(Snapshot, Option<BlockReticleView>, sim::combat::saber::Move)>, String>;

/// The view of seat 1's character in a snapshot of the staged fight,
/// against seat 0's.
#[cfg(feature = "probes")]
fn seat_view(snapshot: &Snapshot) -> Option<BlockReticleView> {
    let objects = Instances::in_snapshot(&sim::Level::arena(), snapshot);
    let blocker = snapshot.body(sim::Seat(1))?;
    let attacker = snapshot.body(sim::Seat(0))?;
    of(snapshot, blocker, &objects, &[attacker])
}

/// The view of seat 1's character in a snapshot of the staged fight
/// changed by `change` (`seat_view`).
#[cfg(feature = "probes")]
fn view_in(snapshot: &Snapshot, change: impl Fn(&mut Snapshot)) -> Option<BlockReticleView> {
    let mut snapshot = snapshot.clone();
    change(&mut snapshot);
    seat_view(&snapshot)
}

/// Seat 1's character in `snapshot`, to change.
#[cfg(feature = "probes")]
fn blocker(snapshot: &mut Snapshot) -> &mut EntityState {
    let id = snapshot.body(sim::Seat(1)).map(|entity| entity.id);
    snapshot
        .entities
        .iter_mut()
        .find(|entity| Some(entity.id) == id)
        .expect("seat 1 has a character")
}

/// The view shows only while the player's own character holds block with a
/// blade, the snapshot's flag is set and the rules are Advanced.
#[cfg(feature = "probes")]
fn view_shown(snapshot: &Snapshot) -> Result<String, String> {
    use sim::combat::{Delivery, saber::SaberRules, sheet::Blaster};
    let blaster = |s: &mut Snapshot| {
        let own = blocker(s);
        let hand = usize::from(own.fighter.map_or(0, |f| f.hand.min(1)));
        if let Some(sheet) = own.sheet.as_mut() {
            sheet.weapons[hand] = Some(sim::combat::Weapon {
                delivery: Delivery::Blaster(Blaster::Pistol),
                ..sim::combat::Weapon::UNARMED
            });
        }
    };
    let released = |s: &mut Snapshot| {
        if let Some(fighter) = blocker(s).fighter.as_mut() {
            fighter.saber.duel.block_held = false;
            fighter.saber.turn = sim::combat::saber::duel::aim::Turn::KEYED;
        }
    };
    type Change<'a> = &'a dyn Fn(&mut Snapshot);
    let rows: [(&str, Change, bool); 5] = [
        ("blocking", &|_| {}, true),
        (
            "Jedi Academy's rules",
            &|s| s.saber_rules = SaberRules::Ja,
            false,
        ),
        ("the flag off", &|s| s.targeted_blocks = false, false),
        ("a blaster in hand", &blaster, false),
        ("block released", &released, false),
    ];
    let got: Vec<(&str, bool)> = rows
        .iter()
        .map(|(name, change, _)| (*name, view_in(snapshot, change).is_some()))
        .collect();
    let want: Vec<(&str, bool)> = rows.iter().map(|(name, _, want)| (*name, *want)).collect();
    match got == want {
        true => Ok(format!("shown as expected: {got:?}")),
        false => Err(format!("shown {got:?}, expected {want:?}")),
    }
}

/// The view follows the flag of the snapshot a guest decodes. `of` takes
/// no setting at all, so the guest's own setting cannot change what it
/// shows: a guest sees the arcs of a host who chose them, and none from a
/// host who did not. The guest's view, its angle included, equals the view
/// of the snapshot the host encoded. With the flag on, the angle of both
/// views is the angle of the last camera turn that the snapshot keeps for
/// seat 1's character.
#[cfg(feature = "probes")]
fn guest_reads_snapshot(snapshot: &Snapshot) -> Result<String, String> {
    let kept = (snapshot.body(sim::Seat(1)))
        .and_then(|own| own.fighter)
        .and_then(|fighter| fighter.saber.turn.aim);
    let views: Vec<(bool, Option<BlockReticleView>, Result<_, String>)> = [true, false]
        .into_iter()
        .map(|flag| {
            let mut host = snapshot.clone();
            host.targeted_blocks = flag;
            let decoded = Snapshot::decode(&host.encode()).map_err(|e| format!("{e:?}"));
            (
                flag,
                seat_view(&host),
                decoded.map(|guest| seat_view(&guest)),
            )
        })
        .collect();
    let wrong: Vec<String> = views
        .iter()
        .filter(|(flag, hosted, guest)| {
            let angled = hosted.is_some_and(|view| view.aim.is_some() && view.aim == kept);
            *guest != Ok(*hosted) || hosted.is_some() != *flag || angled != *flag
        })
        .map(|(flag, hosted, guest)| {
            format!("snapshot flag {flag}: the host's view {hosted:?}, the guest's {guest:?}")
        })
        .collect();
    match wrong.is_empty() {
        true => Ok(format!(
            "`of` takes no setting, and the guest's view equals the host's, at the kept angle {kept:?}: {:?}",
            views
                .iter()
                .map(|(flag, _, guest)| (flag, guest))
                .collect::<Vec<_>>()
        )),
        false => Err(wrong.join("; ")),
    }
}

/// The sides and the angle in the staged fights (`targeted::staged`).
///
/// Seat 0 swings from seat 1's left after seat 1 turned its camera to the
/// left as it raised its guard. The first view to show a swing has the
/// turn and the swing on the left, and the angle of the turn that the
/// snapshot keeps, which is from 225 to 315 degrees, the left quarter of
/// the circle. In the fight where seat 1 has not turned its camera since it
/// raised its guard, every view shown has no side and no angle, and at
/// least one of them shows a swing. A spin attack has no side: every view
/// while it plays shows a swing with no side.
#[cfg(feature = "probes")]
fn view_sides(left: Staged, unturned: Staged, spin: Staged) -> Result<String, String> {
    let kept = |snapshot: &Snapshot| {
        let own = snapshot.body(sim::Seat(1))?;
        own.fighter?.saber.turn.aim
    };
    let first_swing = left?.into_iter().find_map(|(snapshot, view, _)| {
        Some((view.filter(|view| view.swing.is_some())?, kept(&snapshot)))
    });
    let left_ok = first_swing.is_some_and(|(view, kept)| {
        view.turned == Some(Side::Left)
            && view.swing == Some(Swing::Side(Side::Left))
            && view.aim == kept
            && view.aim.is_some_and(|aim| (225.0..=315.0).contains(&aim))
    });
    let still: Vec<BlockReticleView> = unturned?
        .into_iter()
        .filter_map(|(_, view, _)| view)
        .collect();
    let still_swings = still.iter().filter(|view| view.swing.is_some()).count();
    let aimed: Vec<&BlockReticleView> = (still.iter())
        .filter(|view| view.aim.is_some() || view.turned.is_some())
        .collect();
    let spun: Vec<Option<BlockReticleView>> = spin?
        .into_iter()
        .filter(|(_, _, mv)| duel::unblockable(*mv) && aim::swinging(*mv))
        .map(|(_, view, _)| view)
        .collect();
    let spun_wrong: Vec<&Option<BlockReticleView>> = (spun.iter())
        .filter(|view| !view.is_some_and(|view| view.swing == Some(Swing::Any)))
        .collect();
    let still_ok = still_swings > 0 && aimed.is_empty();
    let spins_any = !spun.is_empty() && spun_wrong.is_empty();
    match (left_ok, still_ok, spins_any) {
        (true, true, true) => Ok(format!(
            "the swing from the left, and the view and the kept angle: {first_swing:?}; {} views with no turn since block was raised, {still_swings} of them with a swing, show no side and no angle; {} ticks of the spin show a swing with no side",
            still.len(),
            spun.len()
        )),
        _ => Err(format!(
            "the swing from the left, and the view and the kept angle: {first_swing:?}, expected the left side and the kept angle, from 225 to 315; with no turn since block was raised, {} views shown, {still_swings} with a swing, and these with a side or an angle: {aimed:?}; the spin: {} ticks, these without a swing with no side: {spun_wrong:?}",
            still.len(),
            spun.len()
        )),
    }
}
