//! The mark the player's own blaster shots flash about the crosshair when
//! they land (`ui::screens::hud::hitmarker`): white for a hit, gold for a
//! critical or an assured one, red ticks for a kill, grey for a bolt a
//! blade turned. Blaster shots alone (the owner's call, 2026-11-09): a
//! blade's, a power's or a grenade's hit marks nothing. Several in one
//! step (a flechette's shards) are one mark, the strongest.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::SimId;
use sim::combat::{Delivery, Fighter, Outcome};
use ui::screens::hud::{MARK_SHOWN_MS, Mark};

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

pub struct HitmarkerViewPlugin;

impl Plugin for HitmarkerViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            show.run_if(resource_exists::<PresentedSnapshot>)
                .before(crate::ui_view::Stepped),
        );
    }
}

/// The mark showing, from the presented snapshot's fighters' new events.
fn show(
    presented: Res<PresentedSnapshot>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    mut ui: ResMut<Ui>,
    mut marks: Local<Marks>,
) {
    let snapshot = presented.0.current();
    let generation = swap.map(|swap| swap.generation);
    if marks.generation != generation {
        *marks = Marks {
            generation,
            ..Marks::default()
        };
    }
    let me = crate::cast::me(snapshot, seat.map_or(sim::Seat(0), |seat| seat.0));
    let fighters = snapshot
        .entities
        .iter()
        .filter_map(|entity| Some((entity.id, entity.fighter.as_ref()?)));
    marks.follow(snapshot.time_ms, fighters, me);
    ui.vm.hitmark = marks
        .live
        .map(|(mark, at)| (mark, snapshot.time_ms.saturating_sub(at) as f32))
        .filter(|&(_, age)| age <= MARK_SHOWN_MS);
}

/// What one event shows, the one struck dead after it or not.
pub fn classify(result: Outcome, dead: bool) -> Option<Mark> {
    let strong = match result {
        Outcome::Hit => false,
        Outcome::Critical | Outcome::Automatic => true,
        Outcome::Parried | Outcome::Deflected => return Some(Mark::Turned),
        Outcome::Refused | Outcome::Visual => return None,
    };
    Some(match (dead, strong) {
        (true, strong) => Mark::Kill { strong },
        (false, true) => Mark::Strong,
        (false, false) => Mark::Hit,
    })
}

/// The stronger of two marks: a kill over a strong hit over a hit over a
/// turned bolt; a kill strong if either was.
fn stronger(a: Mark, b: Mark) -> Mark {
    let rank = |mark: Mark| match mark {
        Mark::Turned => 0,
        Mark::Hit => 1,
        Mark::Strong => 2,
        Mark::Kill { .. } => 3,
    };
    match (a, b) {
        (Mark::Kill { strong: x }, Mark::Kill { strong: y }) => Mark::Kill { strong: x || y },
        (Mark::Kill { .. }, Mark::Strong) => Mark::Kill { strong: true },
        (Mark::Strong, Mark::Kill { .. }) => Mark::Kill { strong: true },
        _ if rank(b) > rank(a) => b,
        _ => a,
    }
}

/// The marks of the own shots, followed from the fighters' events.
#[derive(Default)]
pub struct Marks {
    pub generation: Option<u32>,
    /// Each fighter's next event unseen.
    seen: BTreeMap<SimId, u16>,
    /// The mark showing and since when (world ms).
    pub live: Option<(Mark, u64)>,
}

impl Marks {
    /// Follows the `fighters` at `now` (world ms), `me` the own character:
    /// its new blaster hits make the mark.
    pub fn follow<'a>(
        &mut self,
        now: u64,
        fighters: impl Iterator<Item = (SimId, &'a Fighter)>,
        me: Option<SimId>,
    ) {
        let mut step: Option<Mark> = None;
        for (id, fighter) in fighters {
            let Some(last) = self.seen.insert(id, fighter.next_event) else {
                continue;
            };
            let fresh = fighter.next_event.wrapping_sub(last);
            if fresh > u16::MAX / 2 || me.is_none() {
                continue;
            }
            let dead = !fighter.alive();
            let own_bolts = fighter.newest().take(usize::from(fresh)).filter(|event| {
                event.by == me && matches!(event.weapon, Some(Delivery::Blaster(_)))
            });
            for mark in own_bolts.filter_map(|event| classify(event.result, dead)) {
                step = Some(step.map_or(mark, |was| stronger(was, mark)));
            }
        }
        if let Some(mark) = step {
            self.live = Some((mark, now));
        }
    }
}

/// The probes (`kjk sim probes --only hitmarkers`).
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::combat::sheet::Blaster;
    use sim::combat::{Event, Location};
    use sim::probe::check;
    let table = [
        (Outcome::Hit, false, Some(Mark::Hit)),
        (Outcome::Critical, false, Some(Mark::Strong)),
        (Outcome::Automatic, false, Some(Mark::Strong)),
        (Outcome::Hit, true, Some(Mark::Kill { strong: false })),
        (Outcome::Critical, true, Some(Mark::Kill { strong: true })),
        (Outcome::Parried, false, Some(Mark::Turned)),
        (Outcome::Deflected, false, Some(Mark::Turned)),
        (Outcome::Refused, false, None),
        (Outcome::Visual, false, None),
    ];
    let wrong: Vec<_> = table
        .iter()
        .filter(|(result, dead, want)| classify(*result, *dead) != *want)
        .collect();
    let (me, foe, other) = (SimId(1), SimId(2), SimId(3));
    let event = |by: SimId, result, weapon| Event {
        sequence: 0,
        tick: 0,
        by: Some(by),
        result,
        location: Location::None,
        damage: 100,
        weapon,
        at: None,
        visual: None,
    };
    let bolt = Some(Delivery::Blaster(Blaster::Sonic));
    let mut fighters = [me, foe].map(|id| (id, Fighter::fresh(30, 0)));
    let mut marks = Marks::default();
    let follow = |marks: &mut Marks, fighters: &[(SimId, Fighter)], now| {
        marks.follow(now, fighters.iter().map(|(id, f)| (*id, f)), Some(me));
    };
    follow(&mut marks, &fighters, 0);
    // A flechette's five shards, the last killing.
    for _ in 0..4 {
        fighters[1].1.record(event(me, Outcome::Hit, bolt));
    }
    fighters[1].1.record(event(me, Outcome::Critical, bolt));
    fighters[1].1.life = sim::combat::Life::Dead;
    follow(&mut marks, &fighters, 100);
    let volley = marks.live;
    // A blade's hit, and another's bolt: no new mark.
    let mut quiet = Marks::default();
    let mut others = [me, foe].map(|id| (id, Fighter::fresh(30, 0)));
    follow(&mut quiet, &others, 0);
    others[1]
        .1
        .record(event(me, Outcome::Hit, Some(Delivery::Unarmed)));
    others[1].1.record(event(other, Outcome::Hit, bolt));
    follow(&mut quiet, &others, 100);
    vec![
        (
            "classify_table",
            check(wrong.is_empty(), format!("wrong: {wrong:?}")),
        ),
        (
            "marks_merge",
            check(
                volley == Some((Mark::Kill { strong: true }, 100)),
                format!("a killing volley: {volley:?}"),
            ),
        ),
        (
            "only_own_blaster_hits",
            check(
                quiet.live.is_none(),
                format!("a blade's and another's: {:?}", quiet.live),
            ),
        ),
    ]
}
