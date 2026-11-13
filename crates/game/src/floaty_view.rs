//! Floating words over creatures in `kjk play` (render side only, from the
//! presented snapshot): each kill's XP the party gained (`Progress::kills`)
//! over its dead, "XP 25" in KOTOR's magenta, following its head's node on
//! the screen and fading out over `Kill::SHOWN_MS` (`ui::screens::floaty`);
//! the feedback scripts float (`Progress::feedback`, `DisplayFeedBackText`:
//! "Resisted", "Immune", "Saved") over `Feedback::SHOWN_MS`, and a skill
//! check's outcome over its user, starred (`*failure*`: `STARRED`); the damage a
//! hit does, as the K1 executable floats it (`Damages`); none while the one
//! it floats over is off the screen or behind the camera. A sneak attack the
//! player's own character lands floats "Sneak Attack!" over the one struck
//! (`SneaksHeard`), which KOTOR tells only in its combat log. With KOTOR 2's
//! feedback option "Floating Numbers" off, the experience, the damage and
//! the sneak attacks do not float, as KOTOR 2 floats neither of its own then.

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
    /// The player's own character's newest hit on another: on whom, and
    /// when the view saw it (world ms).
    mine: Option<(u32, u64)>,
    /// The "Sneak Attack!" lines this player was told and not yet floated,
    /// each with when it was heard (world ms).
    sneaks: Vec<u64>,
    /// The "Sneak Attack!" words floated: over whom, from when (world ms).
    sneak_shown: Vec<(u32, u64)>,
}

const DAMAGE_COLOR: [f32; 3] = [0.74, 0.11, 0.0];
const DAMAGE_SHOWN_MS: u64 = 1500;

/// How far apart a told sneak attack and the player's hit it belongs to may
/// be seen. The line and the hit come from the same step, but the line
/// arrives as a message while the hit comes in a snapshot, so either may
/// reach this view a few frames before the other.
const SNEAK_PAIR_MS: u64 = 500;

/// The sneak attacks the host told this player since the view last looked
/// (`loot::show` counts them as their lines arrive): KOTOR 2 tells them only
/// in its combat log, and kjk also floats them over the one struck.
#[derive(Resource, Default)]
pub struct SneaksHeard(pub u32);

impl Damages {
    /// Hears `count` sneak attacks told at `now` (world ms).
    fn hear(&mut self, now: u64, count: u32) {
        self.sneaks.extend((0..count).map(|_| now));
    }

    /// Floats each sneak attack heard over the one the player's own
    /// character hit within `SNEAK_PAIR_MS` of it, and forgets those that
    /// found no such hit in that time.
    fn pair_sneaks(&mut self, now: u64) {
        self.sneak_shown
            .retain(|&(_, at)| now.saturating_sub(at) < DAMAGE_SHOWN_MS);
        let mine = self.mine;
        let shown = &mut self.sneak_shown;
        self.sneaks.retain(|&heard| {
            match mine.filter(|&(_, at)| at + SNEAK_PAIR_MS >= heard) {
                Some((over, _)) => {
                    // Two blows of one attack land together: one sign.
                    let fresh = |&(whom, at): &(u32, u64)| {
                        whom == over && now.saturating_sub(at) < SNEAK_PAIR_MS
                    };
                    if !shown.iter().any(fresh) {
                        shown.push((over, now));
                    }
                    false
                }
                None => now.saturating_sub(heard) < SNEAK_PAIR_MS,
            }
        });
    }

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
                .inspect(|event| {
                    if event.by.is_some_and(|by| by != id && Some(by) == me) {
                        self.mine = Some((id.0, now));
                    }
                })
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
        // After the scene's camera has its place for the frame, so the
        // words are drawn where this frame's camera sees their creatures
        // (`ui_view::Floated`).
        app.init_resource::<SneaksHeard>().add_systems(
            PostUpdate,
            (show, floor_labels)
                .chain()
                .run_if(resource_exists::<PresentedSnapshot>)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(crate::ui_view::Floated),
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
    (mut damages, mut heard): (Local<Damages>, ResMut<SneaksHeard>),
    mut facts: ResMut<console::DumpFacts>,
) {
    let snapshot = presented.0.current();
    // What the sim floats (over whom, the strref) and what was drawn.
    if facts.wanted() {
        facts.0.insert(
            "floaties".into(),
            serde_json::json!({
                "feedback": snapshot.progress.feedback.iter()
                    .map(|feedback| [feedback.over, feedback.strref])
                    .collect::<Vec<_>>(),
                "drawn": ui.vm.floaties.iter()
                    .map(|floaty| String::from_utf8_lossy(&floaty.text).into_owned())
                    .collect::<Vec<_>>(),
            }),
        );
    }
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
    damages.hear(snapshot.time_ms, std::mem::take(&mut heard.0));
    damages.pair_sneaks(snapshot.time_ms);
    let progress = &snapshot.progress;
    // KOTOR 2's feedback option "Floating Numbers" off floats no experience
    // and no damage. The scripts' feedback words still float. A sneak
    // attack's words float beside its damage, so they go off with it; the
    // combat log still tells it.
    let numbers = ui::feedback::on(&ui.vm.settings, ui::feedback::FLOATING_NUMBERS);
    let kills: &[Kill] = if numbers { &progress.kills } else { &[] };
    let dealt: &[(u32, i32, u64)] = if numbers { &damages.shown } else { &[] };
    let sneaked: &[(u32, u64)] = if numbers { &damages.sneak_shown } else { &[] };
    let (Some(camera), false) = (
        cameras.single().ok(),
        kills.is_empty() && progress.feedback.is_empty() && dealt.is_empty() && sneaked.is_empty(),
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
    let kills = kills.iter().map(|kill| {
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
        let mut words = words.unwrap_or_else(|| BAD_STRREF.to_vec());
        if STARRED.contains(&feedback.strref) {
            words = [b"*", &words[..], b"*"].concat();
        }
        let color = FEEDBACK_COLOR;
        (
            feedback.over,
            words,
            color,
            feedback.at_ms,
            Feedback::SHOWN_MS,
        )
    });
    let sneak_words =
        text(sim::locks::lines::SNEAK_ATTACK).unwrap_or_else(|| b"Sneak Attack!".to_vec());
    // Before the damage, so the words stand over its number.
    let sneaked = sneaked.iter().map(|&(over, at_ms)| {
        (
            over,
            sneak_words.clone(),
            DAMAGE_COLOR,
            at_ms,
            DAMAGE_SHOWN_MS,
        )
    });
    let dealt = dealt.iter().map(|&(over, hp, at_ms)| {
        let words = hp.to_string().into_bytes();
        (over, words, DAMAGE_COLOR, at_ms, DAMAGE_SHOWN_MS)
    });
    let lines: Vec<_> = kills.chain(feedback).chain(sneaked).chain(dealt).collect();
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

/// The floors stage's labels (`floor_arena::labels`), floated over the
/// patches near the player's own character, in KOTOR's floaty words.
fn floor_labels(
    presented: Res<PresentedSnapshot>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    cameras: Query<(&GlobalTransform, &Camera), SceneCamera>,
    mut ui: ResMut<Ui>,
) {
    if swap.is_none_or(|swap| swap.level.name() != crate::floor_arena::NAME) {
        return;
    }
    let Ok((place, camera)) = cameras.single() else {
        return;
    };
    let snapshot = presented.0.current();
    let me = crate::cast::me(snapshot, seat.map_or(sim::Seat(0), |seat| seat.0))
        .and_then(|id| snapshot.get(id));
    let Some(me) = me else {
        return;
    };
    for (middle, words) in crate::floor_arena::labels(me.position) {
        let at = crate::scene::game_to_bevy() * Vec3::from_array(middle);
        let Some(share) = camera
            .world_to_viewport(place, at)
            .ok()
            .zip(camera.logical_viewport_size())
            .map(|(at, size)| at / size)
            .filter(|share| (0.0..=1.0).contains(&share.x) && (0.0..=1.0).contains(&share.y))
        else {
            continue;
        };
        let lines = words
            .into_iter()
            .enumerate()
            .map(|(row, words)| ui::vm::FloatyView {
                text: words.into_bytes(),
                color: LABEL_COLOR,
                alpha: 1.0,
                over: [share.x, share.y],
                stacked: 1 - row,
            });
        ui.vm.floaties.extend(lines);
    }
}

const LABEL_COLOR: [f32; 3] = [0.85, 0.85, 0.8];

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
    let numbers = (
        "the player's own hits and the hits on it float",
        check(
            shown == [(me.0, 3, 100), (foe.0, 6, 100)] && damages.shown.is_empty(),
            format!("{shown:?}, after 1.5 s {:?}", damages.shown),
        ),
    );
    // A sneak attack told a frame before the player's hit arrives: its
    // words float over the one hit, once for the two blows of one attack.
    damages.hear(2000, 2);
    damages.pair_sneaks(2000);
    fighters[1].1.record(hit(me, 400));
    follow(&mut damages, &fighters, 2100);
    damages.pair_sneaks(2100);
    let paired = damages.sneak_shown.clone();
    // One told with only others' hits near it (the foe's on the player and
    // on another) floats nothing and is forgotten.
    damages.hear(5000, 1);
    fighters[0].1.record(hit(foe, 300));
    fighters[2].1.record(hit(foe, 300));
    follow(&mut damages, &fighters, 5100);
    damages.pair_sneaks(5100);
    follow(&mut damages, &fighters, 5600);
    damages.pair_sneaks(5600);
    let sneaks = (
        "a sneak attack's words float over the one the player hit",
        check(
            paired == [(foe.0, 2100)]
                && damages.sneak_shown.is_empty()
                && damages.sneaks.is_empty(),
            format!(
                "paired {paired:?}; with no hit of the player's: shown {:?}, waiting {:?}",
                damages.sneak_shown, damages.sneaks
            ),
        ),
    );
    vec![numbers, sneaks]
}

/// A skill check's outcome floating over its user (`sim::locks::floats`),
/// which K1's client draws between stars: `*failure*`.
const STARRED: [u32; 4] = {
    use sim::locks::lines;
    [
        lines::SUCCESS,
        lines::FAILURE,
        lines::SKILL_TOO_LOW,
        lines::IMPOSSIBLE,
    ]
};

/// What KOTOR floats for a feedback row with no strref.
const BAD_STRREF: &[u8] = b"BAD STRREF";

/// The colour feedback floats in [verify: KOTOR's `CSWCObject` feedback
/// colour; white here].
const FEEDBACK_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
