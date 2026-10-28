//! Floating words over creatures in `kjk play` (render side only, from the
//! presented snapshot): each kill's XP the party gained (`Progress::kills`)
//! over its dead, "XP 25" in KOTOR's magenta, following its head's node on
//! the screen and fading out over `Kill::SHOWN_MS` (`ui::screens::floaty`);
//! the feedback scripts float (`Progress::feedback`, `DisplayFeedBackText`:
//! "Resisted", "Immune", "Saved") over `Feedback::SHOWN_MS`; the damage a
//! hit does, as the K1 executable floats it (`Damages`); none while the one
//! it floats over is off the screen or behind the camera.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::combat::{Fighter, Outcome};
use sim::{Feedback, Kill, SimId};
use ui::draw::Textures as _;
use ui::screens::floaty::{XP_COLOR, XP_LINE};
use ui::skin::Source;

use crate::cast::Hitboxes;
use crate::lifecycle_view::SceneCamera;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The damage numbers floated, as the K1 executable floats them (its
/// "Floating Numbers" option on): a hit's damage (in whole hit points) in
/// red over the one it lands on when the player's own character dealt it,
/// and over the player's own character whoever dealt it; for
/// `DAMAGE_SHOWN_MS`, from when the view first saw the hit. Any other
/// damage (another's, a script's with no player behind it) floats nothing.
#[derive(Default)]
struct Damages {
    generation: Option<u32>,
    /// Each character's next event unseen.
    seen: BTreeMap<SimId, u16>,
    /// Over whom, how much, from when (world ms).
    shown: Vec<(u32, i32, u64)>,
}

const DAMAGE_COLOR: [f32; 3] = [0.74, 0.11, 0.0];
const DAMAGE_SHOWN_MS: u64 = 1500;

impl Damages {
    /// Follows the `fighters` at `now` (world ms), `me` the player's own
    /// character: their new hits' numbers begin, those shown their time
    /// end.
    fn follow<'a>(
        &mut self,
        now: u64,
        fighters: impl Iterator<Item = (SimId, &'a Fighter)>,
        me: Option<SimId>,
    ) {
        self.shown
            .retain(|&(.., at)| now.saturating_sub(at) < DAMAGE_SHOWN_MS);
        for (id, fighter) in fighters {
            let Some(last) = self.seen.insert(id, fighter.next_event) else {
                continue;
            };
            let fresh = fighter.next_event.wrapping_sub(last);
            if fresh > u16::MAX / 2 {
                continue;
            }
            let hits = fighter.newest().take(usize::from(fresh)).filter(|event| {
                event.damage > 0
                    && matches!(
                        event.result,
                        Outcome::Hit | Outcome::Critical | Outcome::Automatic
                    )
                    && me.is_some_and(|me| event.by == Some(me) || id == me)
            });
            let new: Vec<_> = hits
                .map(|event| (id.0, (event.damage + 50) / 100, now))
                .collect();
            // Newest first in the events; floated in the order dealt.
            self.shown.extend(new.into_iter().rev());
        }
    }
}

pub struct FloatyViewPlugin;

impl Plugin for FloatyViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            show.run_if(resource_exists::<PresentedSnapshot>)
                .before(crate::ui_view::Stepped),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    presented: Res<PresentedSnapshot>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    hitboxes: Option<Res<Hitboxes>>,
    places: Query<&GlobalTransform>,
    cameras: Query<(&GlobalTransform, &Camera), SceneCamera>,
    mut ui: ResMut<Ui>,
    mut damages: Local<Damages>,
) {
    let snapshot = presented.0.current();
    let generation = swap.map(|swap| swap.generation);
    if damages.generation != generation {
        *damages = Damages {
            generation,
            ..Damages::default()
        };
    }
    let me = crate::cast::me(snapshot, seat.map_or(sim::Seat(0), |seat| seat.0));
    let fighters = snapshot
        .entities
        .iter()
        .filter_map(|entity| Some((entity.id, entity.fighter.as_ref()?)));
    damages.follow(snapshot.time_ms, fighters, me);
    let progress = &snapshot.progress;
    let (Some(camera), false) = (
        cameras.single().ok(),
        progress.kills.is_empty() && progress.feedback.is_empty() && damages.shown.is_empty(),
    ) else {
        ui.vm.floaties.clear();
        return;
    };
    let text = |strref: u32| {
        ui.assets
            .as_ref()
            .and_then(|assets| assets.text(Source::Campaign, strref))
            .map(|text| fmt_odyssey::tlk::unbraced(&text))
    };
    let label = text(XP_LINE).unwrap_or_else(|| b"XP".to_vec());
    // Each line: over whom, its words, colour, when and for how long.
    let kills = progress.kills.iter().map(|kill| {
        let mut words = label.clone();
        words.extend(format!(" {}", kill.xp).bytes());
        (kill.dead, words, XP_COLOR, kill.at_ms, Kill::SHOWN_MS)
    });
    let feedback = progress.feedback.iter().map(|feedback| {
        // A row with no strref reads so in KOTOR.
        let words = match feedback.strref {
            0 => None,
            strref => text(strref),
        };
        let words = words.unwrap_or_else(|| BAD_STRREF.to_vec());
        let color = FEEDBACK_COLOR;
        (
            feedback.over,
            words,
            color,
            feedback.at_ms,
            Feedback::SHOWN_MS,
        )
    });
    let dealt = damages.shown.iter().map(|&(over, hp, at_ms)| {
        let words = hp.to_string().into_bytes();
        (over, words, DAMAGE_COLOR, at_ms, DAMAGE_SHOWN_MS)
    });
    let lines: Vec<_> = kills.chain(feedback).chain(dealt).collect();
    let floaties =
        lines
            .iter()
            .enumerate()
            .filter_map(|(at, (whom, words, color, at_ms, shown_ms))| {
                let age = snapshot.time_ms.saturating_sub(*at_ms);
                let id = SimId(*whom);
                let over = crate::bark_view::over(
                    snapshot,
                    id,
                    0.0,
                    hitboxes.as_deref(),
                    &places,
                    camera,
                )?;
                Some(ui::vm::FloatyView {
                    text: words.clone(),
                    color: *color,
                    alpha: 1.0 - age as f32 / *shown_ms as f32,
                    over,
                    stacked: lines[at + 1..]
                        .iter()
                        .filter(|line| line.0 == *whom)
                        .count(),
                })
            });
    ui.vm.floaties = floaties.collect();
}

/// The damage numbers' probes (`kjk sim probes --only "damage numbers"`):
/// the player's own hits float over the struck, any hit on the player
/// over it, others' none; each for 1.5 s.
#[cfg(feature = "probes")]
pub fn damage_probes() -> sim::probe::Checks {
    use sim::combat::{Event, Location};
    use sim::probe::check;
    let (me, foe, other) = (SimId(1), SimId(2), SimId(3));
    let mut fighters = [me, foe, other].map(|id| (id, Fighter::fresh(30, 0)));
    let mut damages = Damages::default();
    let follow = |damages: &mut Damages, fighters: &[(SimId, Fighter)], now| {
        damages.follow(now, fighters.iter().map(|(id, f)| (*id, f)), Some(me));
    };
    follow(&mut damages, &fighters, 0);
    let hit = |by: SimId, damage: i32| Event {
        sequence: 0,
        tick: 0,
        by: Some(by),
        result: Outcome::Hit,
        location: Location::None,
        damage,
        weapon: None,
        at: None,
        visual: None,
    };
    fighters[1].1.record(hit(me, 649));
    fighters[0].1.record(hit(foe, 300));
    fighters[2].1.record(hit(foe, 500));
    follow(&mut damages, &fighters, 100);
    let shown = damages.shown.clone();
    follow(&mut damages, &fighters, 1600);
    vec![(
        "the player's own hits and the hits on it float",
        check(
            shown == [(me.0, 3, 100), (foe.0, 6, 100)] && damages.shown.is_empty(),
            format!("{shown:?}, after 1.5 s {:?}", damages.shown),
        ),
    )]
}

/// What KOTOR floats for a feedback row with no strref.
const BAD_STRREF: &[u8] = b"BAD STRREF";

/// The colour feedback floats in [verify: KOTOR's `CSWCObject` feedback
/// colour; white here].
const FEEDBACK_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
