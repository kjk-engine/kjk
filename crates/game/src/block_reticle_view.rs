//! The arcs of targeted blocks around the centre of the screen
//! (`ui::screens::hud::block_arcs`), from the presented snapshot. They show
//! while the body the player controls holds block with a lightsaber or a
//! melee weapon, and only when the snapshot says targeted blocks apply. The
//! body the player controls is the companion the player leads, if any, and
//! otherwise the player's own character (`Snapshot::body`). The local
//! setting is never read: a guest plays by the host's choice.
//!
//! The view measures the crosshair against the threat, the hostile fighter
//! whose blow will arrive first (`sim::combat::saber::duel::aim::threat`).
//! It reads both sides from `aim::sides`, as the block rule does, so the
//! colours and the rule always agree. The aim arc's angle comes from the
//! same offsets that give the crosshair's side (`aim::offsets`).

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
/// does not play targeted blocks, or while that body is down or does not
/// hold block.
fn show(
    presented: Res<PresentedSnapshot>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    mut ui: ResMut<Ui>,
) {
    let snapshot = presented.0.current();
    let seat = seat.map_or(sim::Seat(0), |seat| seat.0);
    let shown = swap
        .as_ref()
        .filter(|_| snapshot.targeted())
        .zip(snapshot.body(seat).filter(|own| blocking(own)))
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
    ui.vm.block_reticle = shown;
}

/// Whether `own` is alive and holds block.
fn blocking(own: &EntityState) -> bool {
    own.fighter
        .is_some_and(|fighter| fighter.alive() && fighter.saber.duel.block_held)
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
/// targeted blocks do not apply, and while `own` is down, holds no blade or
/// does not hold block.
pub fn of(
    snapshot: &Snapshot,
    own: &EntityState,
    objects: &Instances,
    foes: &[&EntityState],
) -> Option<BlockReticleView> {
    let ps = own.player?;
    if !(snapshot.targeted() && blocking(own) && crate::duel_view::blade_user(own, objects)) {
        return None;
    }
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
        return Some(BlockReticleView::default());
    };
    let (foe, saber) = &foes[threat];
    let (swing, crosshair) = aim::sides(&ps, foe, saber.current_move);
    let (right_deg, up_deg) = aim::offsets(&ps, foe);
    Some(BlockReticleView {
        aim: aim::aim_deg(right_deg, up_deg),
        crosshair: crosshair.map(side),
        swing: near[threat]
            .swing_began
            .map(|_| swing.map_or(Swing::Any, |s| Swing::Side(side(s)))),
    })
}

/// The probes (`kjk sim probes --only "block reticle view"`): when the view
/// shows, that it follows the snapshot's flag and not the local setting,
/// and the sides it shows in a staged fight.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::trace::duelling_probes::targeted::{Stage, staged};
    let staged = |stage| -> Staged {
        let views = staged(stage)?.into_iter().filter_map(|snapshot| {
            let mv = snapshot.body(sim::Seat(0))?.fighter?.saber.current_move;
            let view = seat_view(&snapshot);
            Some((snapshot, view, mv))
        });
        Ok(views.collect())
    };
    let left = staged(Stage::Left);
    let on_chest = staged(Stage::OnChest);
    let spin = staged(Stage::Spin);
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
        ("view_sides", view_sides(left, on_chest, spin)),
    ]
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
/// of the snapshot the host encoded, and with the flag on it has an angle.
#[cfg(feature = "probes")]
fn guest_reads_snapshot(snapshot: &Snapshot) -> Result<String, String> {
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
            let angled = hosted.is_some_and(|view| view.aim.is_some());
            *guest != Ok(*hosted) || hosted.is_some() != *flag || angled != *flag
        })
        .map(|(flag, hosted, guest)| {
            format!("snapshot flag {flag}: the host's view {hosted:?}, the guest's {guest:?}")
        })
        .collect();
    match wrong.is_empty() {
        true => Ok(format!(
            "`of` takes no setting, and the guest's view equals the host's: {:?}",
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
/// Seat 0 swings from seat 1's left while seat 1's crosshair is to the left
/// of seat 0's chest. The first view to show a swing has the crosshair and
/// the swing on the left, and an angle from 225 to 315 degrees, which is
/// the left quarter of the circle. With the crosshair on the chest, every
/// view shown has no side and no angle, and at least one of them shows a
/// swing. A spin attack has no side: every view while it plays shows a
/// swing with no side.
#[cfg(feature = "probes")]
fn view_sides(left: Staged, on_chest: Staged, spin: Staged) -> Result<String, String> {
    let first_swing = left?
        .into_iter()
        .find_map(|(_, view, _)| view.filter(|view| view.swing.is_some()));
    let left_ok = first_swing.is_some_and(|view| {
        view.crosshair == Some(Side::Left)
            && view.swing == Some(Swing::Side(Side::Left))
            && view.aim.is_some_and(|aim| (225.0..=315.0).contains(&aim))
    });
    let centred: Vec<BlockReticleView> = on_chest?
        .into_iter()
        .filter_map(|(_, view, _)| view)
        .collect();
    let centred_swings = centred.iter().filter(|view| view.swing.is_some()).count();
    let off_centre: Vec<&BlockReticleView> = (centred.iter())
        .filter(|view| view.aim.is_some() || view.crosshair.is_some())
        .collect();
    let spun: Vec<Option<BlockReticleView>> = spin?
        .into_iter()
        .filter(|(_, _, mv)| duel::unblockable(*mv) && aim::swinging(*mv))
        .map(|(_, view, _)| view)
        .collect();
    let spun_wrong: Vec<&Option<BlockReticleView>> = (spun.iter())
        .filter(|view| !view.is_some_and(|view| view.swing == Some(Swing::Any)))
        .collect();
    let centred_ok = centred_swings > 0 && off_centre.is_empty();
    let spins_any = !spun.is_empty() && spun_wrong.is_empty();
    match (left_ok, centred_ok, spins_any) {
        (true, true, true) => Ok(format!(
            "the swing from the left: {first_swing:?}; {} views with the crosshair on the chest, {centred_swings} of them with a swing, show no side and no angle; {} ticks of the spin show a swing with no side",
            centred.len(),
            spun.len()
        )),
        _ => Err(format!(
            "the swing from the left: {first_swing:?}, expected the left side and an angle from 225 to 315; with the crosshair on the chest, {} views shown, {centred_swings} with a swing, and these with a side or an angle: {off_centre:?}; the spin: {} ticks, these without a swing with no side: {spun_wrong:?}",
            centred.len(),
            spun.len()
        )),
    }
}
