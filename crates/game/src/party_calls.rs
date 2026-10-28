//! A break and a ready check from the player's side (`sim::party`,
//! `docs/PARTY.md`): the break key (Z) calls a break or ends one (another
//! player's is not ended: the host tells this one so); the in-game menu's
//! break button calls one or ends one this seat may end; the ready key (Y)
//! marks this player ready for the ready check up, or, at the host once it
//! is ready and the check has stood 5 s, forces it. The console's `ui break`,
//! `ui resume`, `ui ready` and `ui force` send those actions as they are.
//! Every screen shows the banner: "Paused by player N" while a break
//! holds, or the ready check's prompt with who is ready. The `paused` and
//! `ready_check` states (`wait paused`), the dump's `calls`, and a log line
//! for each call (`calls: …`) follow the presented snapshot.

use bevy::prelude::*;
use serde_json::json;
use sim::action::Action;
use sim::party::{bit, may_resume};
use sim::{ReadyCheck, Seat, Snapshot};
use ui::vm::{BreakButton, CallsView};

use crate::loot::Outgoing;
use crate::settings::KeyMap;
use crate::sim_host::{OwnSeat, PresentedSnapshot};
use crate::ui_view::Ui;

/// The calls asked for this frame (the console, the keys), to the host.
#[derive(Resource, Default)]
pub struct Asked(pub Vec<Action>);

/// What the presented snapshot said last frame, for the log.
#[derive(Default)]
struct Seen {
    paused_by: Option<Seat>,
    ready: Option<ReadyCheck>,
}

pub struct PartyCallsPlugin;

impl Plugin for PartyCallsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Asked>().add_systems(
            Update,
            (
                show.before(crate::ui_view::Stepped),
                (keys, send).chain().after(crate::ui_view::Stepped),
            ),
        );
    }
}

/// A seat as the screens name it (as `loot::words` names a taker).
pub fn seat_name(seat: Seat) -> String {
    format!("player {}", seat.0 + 1)
}

/// What the break key does: call a break, or end the one that holds (its
/// caller's or the host's resume ends it; anyone else is told it is not
/// theirs, `Notice::NotYourBreak`).
pub fn break_call(snapshot: &Snapshot) -> Action {
    match snapshot.paused_by {
        None => Action::Break,
        Some(_) => Action::Resume,
    }
}

/// What the ready key does for `seat`: ready; at seat 0, already ready and
/// the check standing `FORCE_AFTER_MS`, go now.
pub fn ready_call(snapshot: &Snapshot, seat: Seat) -> Option<Action> {
    let check = snapshot.ready.as_ref()?;
    let old = snapshot.time_ms >= check.since_ms + sim::party::FORCE_AFTER_MS;
    Some(if seat == Seat(0) && check.ready & bit(seat) != 0 && old {
        Action::Force
    } else {
        Action::Ready
    })
}

/// The banner: the break's line, else the ready check's prompt (who asked,
/// where to, who is ready of the seats with a living body, who is waited
/// for and why a seat cannot ready, R11; the rest watch and count as
/// ready).
pub fn banner(snapshot: &Snapshot) -> Option<String> {
    if let Some(by) = snapshot.paused_by {
        return Some(if snapshot.seats() > 1 {
            format!("Paused by {}", seat_name(by))
        } else {
            "Paused".into()
        });
    }
    let check = snapshot.ready.as_ref()?;
    let standing: Vec<Seat> = (0..snapshot.seats())
        .map(Seat)
        .filter(|seat| snapshot.standing(*seat).is_some())
        .collect();
    // A seat waited for, with why it cannot ready if it cannot.
    let named = |seat: Seat, ready: bool| match snapshot.kept_from_exit(seat) {
        Some(why) if !ready => format!("{} ({why})", seat_name(seat)),
        _ => seat_name(seat),
    };
    let names = |ready: bool| {
        standing
            .iter()
            .filter(|seat| (check.ready & bit(**seat) != 0) == ready)
            .map(|seat| named(*seat, ready))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut line = format!(
        "{} wants to go to {}. Ready: {}",
        seat_name(check.asked_by),
        check.module,
        names(true)
    );
    let waiting = names(false);
    if !waiting.is_empty() {
        line.push_str(&format!("; waiting for {waiting}"));
    }
    line.push('.');
    if snapshot.in_combat {
        line.push_str(" Enemies nearby.");
    }
    Some(line)
}

/// What the menu's break button does for `seat`.
fn button(snapshot: &Snapshot, seat: Seat) -> BreakButton {
    match (snapshot.paused_by, snapshot.in_combat) {
        (Some(by), _) if may_resume(by, seat) => BreakButton::Resume,
        (Some(_), _) | (None, true) => BreakButton::Off,
        (None, false) => BreakButton::Break,
    }
}

/// The break and ready keys in play, with no screen open.
fn keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<KeyMap>>,
    ui: Option<Res<Ui>>,
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    mut asked: ResMut<Asked>,
) {
    let (Some(keys), Some(presented), Some(own)) = (keys, presented, own) else {
        return;
    };
    if ui.is_some_and(|ui| ui.stack.is_open()) {
        return;
    }
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let snapshot = presented.0.current();
    if map.pressed_now(&keys, crate::settings::Action::Break) {
        asked.0.push(break_call(snapshot));
    }
    if map.pressed_now(&keys, crate::settings::Action::Ready) {
        asked.0.extend(ready_call(snapshot, own.0));
    }
}

fn send(mut asked: ResMut<Asked>, outgoing: Option<ResMut<Outgoing>>) {
    if let Some(mut outgoing) = outgoing {
        outgoing.0.append(&mut asked.0);
    }
}

/// The banner and the menu's button, the states, the dump and the log,
/// from the presented snapshot.
fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    ui: Option<ResMut<Ui>>,
    mut states: ResMut<console::States>,
    mut facts: ResMut<console::DumpFacts>,
    mut seen: Local<Seen>,
) {
    let calls = presented
        .as_ref()
        .map_or_else(CallsView::default, |presented| {
            let snapshot = presented.0.current();
            CallsView {
                banner: banner(snapshot),
                button: own
                    .as_ref()
                    .map_or(BreakButton::Break, |own| button(snapshot, own.0)),
            }
        });
    let shown_banner = calls.banner.clone();
    if let Some(mut ui) = ui {
        ui.vm.calls = calls;
    }
    let Some(presented) = presented else {
        return;
    };
    let snapshot = presented.0.current();
    states
        .0
        .insert("paused".into(), snapshot.paused_by.is_some());
    states
        .0
        .insert("ready_check".into(), snapshot.ready.is_some());
    if snapshot.paused_by != seen.paused_by {
        match snapshot.paused_by {
            Some(by) => info!(
                target: "party",
                "calls: break by {} at tick {}",
                seat_name(by),
                snapshot.tick
            ),
            None => info!(target: "party", "calls: resumed at tick {}", snapshot.tick),
        }
    }
    match (&seen.ready, &snapshot.ready) {
        (None, Some(check)) => info!(
            target: "party",
            "calls: ready check by {} to {} at tick {}",
            seat_name(check.asked_by),
            check.module,
            snapshot.tick
        ),
        (Some(check), None) => {
            let went = presented
                .1
                .iter()
                .any(|stepped| stepped.transition.is_some());
            info!(
                target: "party",
                "calls: ready check to {} {} at tick {}",
                check.module,
                if went { "went" } else { "lapsed" },
                snapshot.tick
            );
        }
        _ => {}
    }
    facts.0.insert(
        "calls".into(),
        json!({
            "banner": shown_banner,
            "in_combat": snapshot.in_combat,
            "paused_by": snapshot.paused_by.map(|seat| seat.0),
            "ready": snapshot.ready.as_ref().map(|check| json!({
                "module": check.module,
                "asked_by": check.asked_by.0,
                "ready": check.ready,
                "since_ms": check.since_ms,
            })),
        }),
    );
    *seen = Seen {
        paused_by: snapshot.paused_by,
        ready: snapshot.ready.clone(),
    };
}
