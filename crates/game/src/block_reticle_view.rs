//! The marks of targeted blocks around the crosshair
//! (`ui::screens::hud::block_marks`), from the presented snapshot. They show
//! while the player's own character holds block with a lightsaber or a
//! melee weapon, and only when the snapshot says targeted blocks apply. The
//! local setting is never read: a guest plays by the host's choice.
//!
//! The view measures the crosshair against the threat, the hostile fighter
//! whose blow will arrive first (`sim::combat::saber::duel::aim::threat`).
//! It reads both sides from `aim::sides`, as the block rule does, so the
//! marks and the rule always agree.

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

/// The marks shown, from the presented snapshot's own character and the
/// fighters hostile to it. Nothing is looked up in a session that does not
/// play targeted blocks.
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
        .zip(snapshot.body(seat))
        .and_then(|(swap, own)| {
            let objects = Instances::in_snapshot(&swap.level, snapshot);
            let standing = swap.level.module().map(|module| snapshot.standings(module));
            let hostile = crate::battle_music::hostility(standing);
            let (bodies, _) = crate::battle_music::bodies_and_blows(snapshot, &objects);
            let body = |id: sim::SimId| bodies.iter().find(|body| body.id == id.0);
            let mine = body(own.id)?;
            let foes: Vec<&EntityState> = snapshot
                .entities
                .iter()
                .filter(|entity| body(entity.id).is_some_and(|it| it.alive && hostile(it, mine)))
                .collect();
            of(snapshot, own, &objects, &foes)
        });
    ui.vm.block_reticle = shown;
}

/// The side as the HUD names it.
fn side(side: aim::Side) -> Side {
    match side {
        aim::Side::Left => Side::Left,
        aim::Side::Right => Side::Right,
        aim::Side::Above => Side::Above,
    }
}

/// What the HUD draws around the crosshair of `own` in `snapshot`, with
/// `objects` holding the snapshot's sheets, against the hostile fighters
/// `foes`. None while targeted blocks do not apply, and while `own` is
/// down, holds no blade or does not hold block.
pub fn of(
    snapshot: &Snapshot,
    own: &EntityState,
    objects: &Instances,
    foes: &[&EntityState],
) -> Option<BlockReticleView> {
    let ps = own.player?;
    let fighter = own.fighter?;
    let blocking = fighter.alive() && fighter.saber.duel.block_held;
    if !(snapshot.targeted() && blocking && crate::duel_view::blade_user(own, objects)) {
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
    Some(BlockReticleView {
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
    let staged = |spin| -> Staged {
        let ticks = sim::trace::duelling_probes::targeted::staged(spin)?;
        let level = sim::Level::arena();
        let views = ticks.into_iter().filter_map(|snapshot| {
            let blocker = *snapshot.body(sim::Seat(1))?;
            let attacker = *snapshot.body(sim::Seat(0))?;
            let objects = Instances::in_snapshot(&level, &snapshot);
            let view = of(&snapshot, &blocker, &objects, &[&attacker]);
            let mv = attacker.fighter?.saber.current_move;
            Some((snapshot, view, mv))
        });
        Ok(views.collect())
    };
    let left = staged(false);
    let spin = staged(true);
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
        ("view_sides", view_sides(left, spin)),
    ]
}

/// The views of seat 1's character against seat 0's in a staged fight, each
/// with its snapshot and seat 0's move.
#[cfg(feature = "probes")]
type Staged = Result<Vec<(Snapshot, Option<BlockReticleView>, sim::combat::saber::Move)>, String>;

/// A snapshot of the staged fight changed by `change`, and the view of seat
/// 1's character in it against seat 0's.
#[cfg(feature = "probes")]
fn view_in(snapshot: &Snapshot, change: impl Fn(&mut Snapshot)) -> Option<BlockReticleView> {
    let mut snapshot = snapshot.clone();
    change(&mut snapshot);
    let level = sim::Level::arena();
    let objects = Instances::in_snapshot(&level, &snapshot);
    let blocker = snapshot.body(sim::Seat(1))?;
    let attacker = snapshot.body(sim::Seat(0))?;
    of(&snapshot, blocker, &objects, &[attacker])
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

/// The view follows the flag of the snapshot a guest decodes, whatever the
/// guest's own setting says. The view takes no setting at all, so a guest
/// whose setting is off sees the marks of a host who chose them, and a guest
/// whose setting is on sees none from a host who did not.
#[cfg(feature = "probes")]
fn guest_reads_snapshot(snapshot: &Snapshot) -> Result<String, String> {
    let local = crate::settings::Settings::defaults().targeted_blocks();
    let rows = [(false, true, true), (true, false, false)];
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|&(local_on, flag, want)| {
            let mut host = snapshot.clone();
            host.targeted_blocks = flag;
            let decoded = Snapshot::decode(&host.encode()).map_err(|e| format!("{e:?}"));
            let shown = decoded.map(|guest| view_in(&guest, |_| {}).is_some());
            (shown != Ok(want))
                .then(|| format!("local setting {local_on}, snapshot flag {flag}: shown {shown:?}"))
        })
        .collect();
    match (local, wrong.is_empty()) {
        (false, true) => Ok("the view follows the snapshot's flag".into()),
        (true, _) => Err("the default local setting is on".into()),
        (false, false) => Err(wrong.join("; ")),
    }
}

/// The sides in the staged fights (`targeted::staged`). Seat 0 swings from
/// seat 1's left while seat 1's crosshair is to the left of seat 0's chest:
/// the first view to show a swing has the crosshair and the swing on the
/// left. A spin attack has no side: every view while it plays shows a swing
/// with no side.
#[cfg(feature = "probes")]
fn view_sides(left: Staged, spin: Staged) -> Result<String, String> {
    let first_swing = left?
        .into_iter()
        .find_map(|(_, view, _)| view.filter(|view| view.swing.is_some()));
    let spun: Vec<Option<BlockReticleView>> = spin?
        .into_iter()
        .filter(|(_, _, mv)| duel::unblockable(*mv) && aim::swinging(*mv))
        .map(|(_, view, _)| view)
        .collect();
    let want = BlockReticleView {
        crosshair: Some(Side::Left),
        swing: Some(Swing::Side(Side::Left)),
    };
    let spins_any = !spun.is_empty()
        && spun
            .iter()
            .all(|view| view.is_some_and(|view| view.swing == Some(Swing::Any)));
    match (first_swing == Some(want), spins_any) {
        (true, true) => Ok(format!(
            "the swing from the left: {first_swing:?}; {} ticks of the spin show a swing with no side",
            spun.len()
        )),
        _ => Err(format!(
            "the swing from the left: {first_swing:?}, expected {want:?}; the spin: {spun:?}"
        )),
    }
}
