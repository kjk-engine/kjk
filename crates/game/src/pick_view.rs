//! Which kind of item each key uses, from the player's side
//! (`sim::combat::picks`): hold an item key and the wheel (a pad's RB) moves
//! through the kinds carried, sending `Action::PickItem`, and a label over
//! the slot names the one chosen; let go having moved and nothing is used.
//!
//! A grenade's key charges from its press, as ever; the first step drops the
//! charge in the sim (`Pack::charge_off`). The others are used on a press,
//! which leaves no time to scroll, so with two kinds or more the press is
//! held back and a tap uses the kind as it is let go.

use std::sync::Arc;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use sim::action::Action;
use sim::combat::items::{Shelf, use_of};
use sim::combat::{Fighter, Sheet};
use sim::inventory::PARTY;
use sim::notice::Notice;
use sim::{SimId, Snapshot};

use crate::loot::{Outgoing, words};
use crate::pad::Actions;
use crate::settings::Action as Key;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The item keys, by `Shelf`.
pub const KEYS: [(Shelf, Key); Shelf::ALL.len()] = [
    (Shelf::Medpac, Key::Medpac),
    (Shelf::Stim, Key::Stim),
    (Shelf::Shield, Key::Shield),
    (Shelf::Grenade, Key::Grenade),
];

/// How long a tap's press is held down for the sim to see it (seconds: some
/// of its steps, whatever the frame rate).
const TAP: f32 = 0.04;
/// How long a grenade's key stays down after a let-go, at most, for the sim
/// to take the pick first: a let-go alone would throw (seconds).
const SETTLE: f32 = 0.25;
/// How often a pick that should have dropped the charge is sent again while
/// the snapshots show no drop (seconds): a joined player's pick can reach the
/// host ahead of its press, which then has no charge to drop.
const RESEND: f32 = 0.1;
/// How long a pick not yet seen in a snapshot is believed (seconds).
const BELIEVED: f32 = 1.0;
/// How long a label shows after the last change, and the fade at its end
/// (seconds).
const SHOWN: f32 = 1.5;
const FADE: f32 = 0.5;
/// A trackpad's pixels to a wheel's notch.
const NOTCH_PIXELS: f32 = 40.0;

/// A kind of item a key can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    /// Its `spells.2da` row.
    pub row: u16,
    pub left: u32,
    /// An item that casts it; none where the goods don't say (an arena's
    /// sheet).
    pub template: Option<String>,
}

/// What a label stands over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelOn {
    Item(Shelf),
    Feat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hold {
    shelf: Shelf,
    moved: bool,
    /// Its press is held back until it is let go.
    deferred: bool,
    /// It has kinds to choose among.
    choice: bool,
    /// A screen or another body came between: the key does nothing more
    /// until it is let go.
    cancelled: bool,
}

/// A frame of the keys, for `Picking::frame`.
#[derive(Debug, Clone, Copy, Default)]
struct Frame {
    held: [bool; KEYS.len()],
    pressed: [bool; KEYS.len()],
    /// How many kinds each key has.
    counts: [usize; KEYS.len()],
    steps: i32,
    /// The sim took the pick that drops the charge (`Pack::charge_off`).
    dropped: bool,
    seconds: f32,
}

/// What `Picking::frame` makes of it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Keyed {
    /// A key with kinds to choose among was pressed, or is moving.
    shelf: Option<Shelf>,
    steps: i32,
    /// Each key's state where it differs from the device's.
    set: [Option<bool>; KEYS.len()],
}

/// The item key held, the presses due, what the player picked and what the
/// label says.
#[derive(Resource, Debug, Default)]
pub struct Picking {
    hold: Option<Hold>,
    tap: [f32; KEYS.len()],
    settling: f32,
    /// What the wheel has turned short of a notch.
    spare: f32,
    /// A pick the snapshots don't show yet: its row and when (seconds).
    sent: [Option<(u16, f32)>; KEYS.len()],
    /// When a pick last went out (seconds).
    last_sent: f32,
    /// The body the keys last played.
    body: Option<SimId>,
    label: Option<(LabelOn, Vec<u8>, f32)>,
    /// The keys a run's commands hold (`ui key`), the frame before's, the
    /// wheel they turned and the next-feat key they pressed: a headless run
    /// reads no device.
    held_by_script: [bool; KEYS.len()],
    was_by_script: [bool; KEYS.len()],
    wheel_by_script: f32,
    feat_by_script: bool,
    /// A run's commands took the keys: its commands count though it has no
    /// window (`PlayerInput::read`).
    keys_scripted: bool,
}

impl Picking {
    /// An item key is held: it has the wheel.
    pub fn holding(&self) -> bool {
        self.hold.is_some()
    }

    /// The key held that has kinds to choose among (`pad`: it stays held
    /// whatever the second layer does, and RB steps).
    pub(crate) fn choosing(&self) -> Option<Key> {
        let hold = self.hold.filter(|hold| hold.choice && !hold.cancelled)?;
        Some(KEYS[hold.shelf as usize].1)
    }

    /// Nothing is going on: no key held, no press due, no pick on its way.
    fn idle(&self, actions: &Actions) -> bool {
        self.hold.is_none()
            && self.settling <= 0.0
            && self.tap.iter().all(|tap| *tap <= 0.0)
            && self.sent.iter().all(Option::is_none)
            && KEYS.iter().all(|(_, key)| !actions.held(*key))
    }

    /// A pick should have dropped the charge, and the snapshots show it hasn't.
    fn drop_due(&self, dropped: bool) -> bool {
        let held = self
            .hold
            .is_some_and(|hold| hold.shelf == Shelf::Grenade && hold.moved && !hold.cancelled);
        !dropped && (held || self.settling > 0.0)
    }

    /// The kind just picked for `shelf`, while the snapshots catch up.
    pub fn pending(&self, shelf: Shelf) -> Option<u16> {
        self.sent[shelf as usize].map(|(row, _)| row)
    }

    /// `words` over `on`, from `now` (seconds of the real clock).
    pub fn say(&mut self, on: LabelOn, words: Vec<u8>, now: f32) {
        self.label = Some((on, words, now));
    }

    /// The label at `now`: over what, its words and opacity.
    pub fn label(&self, now: f32) -> Option<(LabelOn, Vec<u8>, f32)> {
        let (on, words, at) = self.label.as_ref()?;
        let alpha = ((at + SHOWN - now) / FADE).clamp(0.0, 1.0);
        (alpha > 0.0).then(|| (*on, words.clone(), alpha))
    }

    /// A run's commands hold keys, which a headless run has otherwise none of.
    pub(crate) fn keys_scripted(&self) -> bool {
        self.keys_scripted
    }

    /// A command's request: `key hold grenade`, `key up grenade`, `key feat`
    /// or `wheel 1` (notches, down).
    pub(crate) fn script(&mut self, words: &[&str]) -> bool {
        let on = |name: &str| {
            Some(match name {
                "medpac" => Shelf::Medpac,
                "stim" => Shelf::Stim,
                "shield" => Shelf::Shield,
                "grenade" => Shelf::Grenade,
                _ => return None,
            })
        };
        match words {
            ["key", "feat"] => self.feat_by_script = true,
            ["key", verb @ ("hold" | "up"), name] => match on(name) {
                Some(shelf) => self.held_by_script[shelf as usize] = *verb == "hold",
                None => return false,
            },
            ["wheel", notches] => match notches.parse::<f32>() {
                Ok(notches) => self.wheel_by_script -= notches,
                Err(_) => return false,
            },
            _ => return false,
        }
        self.keys_scripted = true;
        true
    }

    /// What the run's commands hold, put in `actions`; the wheel they turned.
    fn scripted(&mut self, actions: &mut Actions) -> f32 {
        for ((shelf, key), held) in KEYS.into_iter().zip(self.held_by_script) {
            if held {
                actions.set_held(key, true);
                if !self.was_by_script[shelf as usize] {
                    actions.set_pressed(key);
                }
            }
        }
        if std::mem::take(&mut self.feat_by_script) {
            actions.set_pressed(Key::NextFeat);
        }
        self.was_by_script = self.held_by_script;
        std::mem::take(&mut self.wheel_by_script)
    }

    /// A screen or another body came between: what is due is dropped, and a
    /// key scrolled or held back does nothing more until it is let go.
    fn cancel(&mut self) {
        self.hold = self
            .hold
            .filter(|hold| hold.deferred || hold.moved)
            .map(|hold| Hold {
                cancelled: true,
                ..hold
            });
        self.tap = Default::default();
        self.settling = 0.0;
        self.spare = 0.0;
    }

    fn frame(&mut self, frame: Frame) -> Keyed {
        let mut out = Keyed::default();
        for (tap, set) in self.tap.iter_mut().zip(&mut out.set) {
            if *tap > 0.0 {
                *set = Some(true);
                *tap -= frame.seconds;
            }
        }
        let grenade = Shelf::Grenade as usize;
        if self.settling > 0.0 {
            if frame.dropped {
                self.settling = 0.0;
            } else {
                out.set[grenade] = Some(true);
                self.settling -= frame.seconds;
            }
        }
        match self.hold {
            Some(hold) if hold.cancelled => {
                let at = hold.shelf as usize;
                if frame.held[at] {
                    out.set[at] = Some(false);
                } else {
                    self.hold = None;
                }
            }
            Some(mut hold) => {
                let at = hold.shelf as usize;
                if frame.held[at] {
                    if frame.steps != 0 && frame.counts[at] >= 2 {
                        hold.moved = true;
                        out.steps = frame.steps;
                    }
                    if hold.deferred {
                        out.set[at] = Some(false);
                    }
                    out.shelf = (frame.counts[at] >= 2).then_some(hold.shelf);
                    self.hold = Some(hold);
                } else {
                    if hold.deferred && !hold.moved {
                        self.tap[at] = TAP;
                        out.set[at] = Some(true);
                    }
                    if at == grenade && hold.moved && !frame.dropped {
                        self.settling = SETTLE;
                        out.set[at] = Some(true);
                    }
                    self.hold = None;
                }
            }
            None => {
                if let Some(shelf) = Shelf::ALL
                    .into_iter()
                    .find(|shelf| frame.pressed[*shelf as usize])
                {
                    let at = shelf as usize;
                    let choice = frame.counts[at] >= 2;
                    let deferred = choice && shelf != Shelf::Grenade;
                    self.hold = Some(Hold {
                        shelf,
                        moved: false,
                        deferred,
                        choice,
                        cancelled: false,
                    });
                    if deferred {
                        out.set[at] = Some(false);
                    }
                    out.shelf = choice.then_some(shelf);
                }
            }
        }
        out
    }
}

/// The kinds each key can use (by `Shelf`), in the order the goods hold them:
/// what its holder carries or it wears, for its race (as
/// `sim::inventory::kinds` has them, of which `Sheet::items` holds some),
/// and any more its sheet shows, its uses not yet taken from them left out.
pub fn kinds(
    snapshot: &Snapshot,
    body: SimId,
    sheet: &Sheet,
    fighter: &Fighter,
    library: Option<&Arc<sim::script::Library>>,
) -> [Vec<Kind>; KEYS.len()] {
    let held: Vec<_> = library.map_or_else(Vec::new, |library| {
        let catalog = library.items();
        snapshot
            .goods
            .items
            .iter()
            .filter(|(_, item)| item.holder == PARTY || item.holder == body.0)
            .filter_map(|(_, item)| {
                let facts = snapshot.goods.facts(catalog, &item.template)?;
                facts.ready(item.slot.is_some()).then_some((
                    item.uses(facts),
                    facts,
                    item.template.as_str(),
                ))
            })
            .collect()
    });
    let carried = library.map_or_else(Vec::new, |library| {
        sim::inventory::kinds(
            sheet.race,
            held.iter().map(|(uses, facts, _)| (*uses, *facts)),
            |row| library.impact(row).is_some(),
        )
    });
    let shown = sheet.items.iter().flatten().copied();
    let rows = carried.iter().copied().chain(
        shown
            .clone()
            .filter(|kind| carried.iter().all(|known| known.row != kind.row)),
    );
    let mut out: [Vec<Kind>; KEYS.len()] = Default::default();
    for kind in rows {
        let Some(on) = use_of(kind.row) else {
            continue;
        };
        let slot = sheet
            .items
            .iter()
            .position(|item| item.is_some_and(|item| item.row == kind.row));
        let spent = slot.map_or(0, |at| fighter.pack.used[at]);
        let left = u32::from(kind.count.saturating_sub(spent));
        if left == 0 {
            continue;
        }
        let template = held
            .iter()
            .find(|(_, facts, _)| facts.combat_row() == Some(kind.row))
            .map(|(_, _, template)| template.to_string());
        out[on.shelf() as usize].push(Kind {
            row: kind.row,
            left,
            template,
        });
    }
    out
}

/// Which of `kinds[shelf]` the key uses now: the one just picked, or what the
/// sim's key would take (`Pack::keyed`), else its first.
pub fn chosen(
    kinds: &[Vec<Kind>; KEYS.len()],
    sheet: &Sheet,
    fighter: &Fighter,
    shelf: Shelf,
    pending: Option<u16>,
    now_ms: u64,
) -> Option<usize> {
    let list = &kinds[shelf as usize];
    pending
        .or_else(|| {
            let (_, kind) = fighter.pack.keyed(sheet, shelf, now_ms)?;
            Some(kind.row)
        })
        .and_then(|row| list.iter().position(|kind| kind.row == row))
        .or((!list.is_empty()).then_some(0))
}

/// What the label says of `kind`: its item's name and the uses left.
fn named(ui: &mut Ui, kind: &Kind) -> Option<Vec<u8>> {
    let name = kind.template.as_ref().and_then(|template| {
        let look = ui.assets.as_mut()?.item_ref(template)?;
        Some(look.name.clone())
    });
    let mut name = name.or_else(|| {
        let strref = ui
            .assets
            .as_mut()?
            .cell("spells", usize::from(kind.row), "name")?
            .parse()
            .ok()?;
        Some(words(&Notice::Line(strref), Some(ui)).into_bytes())
    })?;
    name.extend(format!("  x{}", kind.left).into_bytes());
    Some(name)
}

/// A screen, a conversation or a movie has the keys.
pub(crate) fn busy(
    ui: Option<&Ui>,
    replies: Option<&crate::dialogue_view::ReplyInput>,
    showing: Option<&crate::movie_view::Showing>,
) -> bool {
    ui.is_some_and(|ui| ui.stack.is_open())
        || replies.is_some_and(|replies| replies.active)
        || showing.is_some_and(|showing| showing.0)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pick(
    mut actions: ResMut<Actions>,
    mut wheel: MessageReader<MouseWheel>,
    // `leader_view::Busy` but the screens, which this names words with.
    (replies, showing): (
        Option<Res<crate::dialogue_view::ReplyInput>>,
        Option<Res<crate::movie_view::Showing>>,
    ),
    (presented, own, swap): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
    ),
    (mut picking, mut outgoing, mut ui): (
        ResMut<Picking>,
        Option<ResMut<Outgoing>>,
        Option<ResMut<Ui>>,
    ),
    time: Res<Time<Real>>,
) {
    let turned: f32 = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / NOTCH_PIXELS,
        })
        .sum();
    let turned = turned + picking.scripted(&mut actions);
    if picking.idle(&actions) {
        return;
    }
    let busy = busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    let body = (!busy)
        .then_some(())
        .and_then(|()| Some((presented?, own?, swap?)));
    let body = body.as_ref().and_then(|(presented, own, swap)| {
        let snapshot = presented.0.current();
        let (body, sheet) = crate::cast::body_sheet(snapshot, own.0, &swap.level)?;
        let fighter = body.fighter?;
        let library = swap.level.scripts();
        let kinds = kinds(snapshot, body.id, &sheet, &fighter, library);
        Some((snapshot.time_ms, body.id, sheet, fighter, kinds))
    });
    let Some((now_ms, id, sheet, fighter, kinds)) = body else {
        picking.cancel();
        return;
    };
    if picking.body.replace(id).is_some_and(|was| was != id) {
        picking.cancel();
    }
    let now = time.elapsed_secs();
    for (shelf, _) in KEYS {
        let sent = &mut picking.sent[shelf as usize];
        if let Some((row, at)) = *sent
            && (sheet.picks.item(shelf) == Some(row) || now - at > BELIEVED)
        {
            *sent = None;
        }
    }
    picking.spare = if picking.holding() {
        picking.spare + turned
    } else {
        0.0
    };
    // The wheel up goes back, as on the target's actions (`target_view`).
    let notches = picking.spare.trunc();
    picking.spare -= notches;
    let frame = Frame {
        held: KEYS.map(|(_, key)| actions.held(key)),
        pressed: KEYS.map(|(_, key)| actions.pressed(key)),
        counts: std::array::from_fn(|at| kinds[at].len()),
        steps: i32::from(actions.pick_step) - notches as i32,
        dropped: fighter.pack.charge_off,
        seconds: time.delta_secs(),
    };
    let keyed = picking.frame(frame);
    for ((_, key), set) in KEYS.iter().zip(keyed.set) {
        if let Some(on) = set {
            actions.set_held(*key, on);
        }
    }
    let shelf = Shelf::Grenade;
    if picking.drop_due(fighter.pack.charge_off)
        && now - picking.last_sent > RESEND
        && let Some(row) = picking.pending(shelf).or(sheet.picks.item(shelf))
        && let Some(outgoing) = outgoing.as_mut()
    {
        outgoing.0.push(Action::PickItem { row });
        picking.last_sent = now;
    }
    let Some(shelf) = keyed.shelf else {
        return;
    };
    let list = &kinds[shelf as usize];
    let pending = picking.pending(shelf);
    let Some(at) = chosen(&kinds, &sheet, &fighter, shelf, pending, now_ms) else {
        return;
    };
    let at = (at as i32 + keyed.steps).rem_euclid(list.len() as i32) as usize;
    let kind = &list[at];
    if keyed.steps != 0
        && let Some(outgoing) = outgoing.as_mut()
    {
        outgoing.0.push(Action::PickItem { row: kind.row });
        picking.sent[shelf as usize] = Some((kind.row, now));
        picking.last_sent = now;
    }
    if let Some(words) = ui.as_mut().and_then(|ui| named(ui, kind)) {
        picking.say(LabelOn::Item(shelf), words, now);
    }
}

/// The item keys are settled in `Actions`: what steps the sim comes after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Picked;

pub struct PickViewPlugin;

impl Plugin for PickViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picking>()
            .add_systems(PreUpdate, pick.in_set(Picked).after(crate::pad::Sampled));
    }
}

/// The keys' rules, no window (`kjk sim probes`, group "item_keys").
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    vec![
        ("one kind uses on the press", one_kind_on_press()),
        ("two kinds use on the let-go", two_kinds_on_let_go()),
        ("scrolling uses nothing", scrolling_uses_nothing()),
        ("one kind has nothing to scroll", one_kind_no_scroll()),
        ("a grenade charges from its press", grenade_charges()),
        ("a grenade scrolled waits for the sim", grenade_waits()),
        ("the label fades", label_fades()),
        ("a cancelled hold does nothing more", cancelled_hold()),
    ]
}

/// Runs `script` (the key's state, the wheel's steps) on `shelf`'s key with
/// `count` kinds, then two frames with the key up: what the sim sees of the
/// key each frame, and the steps taken.
#[cfg(feature = "probes")]
fn run(shelf: Shelf, count: usize, script: &[(bool, i32)], dropped: bool) -> (Vec<bool>, Vec<i32>) {
    let mut picking = Picking::default();
    let (mut seen, mut moved) = (Vec::new(), Vec::new());
    let at = shelf as usize;
    let mut was = false;
    for (held, steps) in script.iter().chain(&[(false, 0); 2]) {
        let mut frame = Frame {
            steps: *steps,
            dropped,
            seconds: 0.01,
            ..Frame::default()
        };
        frame.held[at] = *held;
        frame.pressed[at] = *held && !was;
        frame.counts[at] = count;
        let keyed = picking.frame(frame);
        seen.push(keyed.set[at].unwrap_or(*held));
        moved.push(keyed.steps);
        was = *held;
    }
    (seen, moved)
}

/// How many separate presses the sim sees.
#[cfg(feature = "probes")]
fn presses(seen: &[bool]) -> usize {
    let was = std::iter::once(&false).chain(seen);
    seen.iter()
        .zip(was)
        .filter(|(on, was)| **on && !**was)
        .count()
}

#[cfg(feature = "probes")]
const TAPPED: [(bool, i32); 2] = [(true, 0), (true, 0)];
#[cfg(feature = "probes")]
const SCROLLED: [(bool, i32); 3] = [(true, 0), (true, 1), (true, 0)];

#[cfg(feature = "probes")]
fn one_kind_on_press() -> Result<String, String> {
    let (seen, _) = run(Shelf::Medpac, 1, &TAPPED, false);
    sim::probe::check(seen[0] && presses(&seen) == 1, format!("{seen:?}"))
}

#[cfg(feature = "probes")]
fn two_kinds_on_let_go() -> Result<String, String> {
    let (seen, _) = run(Shelf::Stim, 2, &TAPPED, false);
    sim::probe::check(
        !seen[0] && !seen[1] && seen[2] && presses(&seen) == 1,
        format!("{seen:?}"),
    )
}

#[cfg(feature = "probes")]
fn scrolling_uses_nothing() -> Result<String, String> {
    let (seen, moved) = run(Shelf::Shield, 3, &SCROLLED, false);
    sim::probe::check(
        presses(&seen) == 0 && moved == [0, 1, 0, 0, 0],
        format!("{seen:?} {moved:?}"),
    )
}

#[cfg(feature = "probes")]
fn one_kind_no_scroll() -> Result<String, String> {
    let (seen, moved) = run(Shelf::Stim, 1, &SCROLLED, false);
    sim::probe::check(
        presses(&seen) == 1 && moved.iter().all(|step| *step == 0),
        format!("{seen:?} {moved:?}"),
    )
}

#[cfg(feature = "probes")]
fn grenade_charges() -> Result<String, String> {
    let (seen, _) = run(Shelf::Grenade, 2, &TAPPED, false);
    sim::probe::check(seen[0] && seen[1] && !seen[2], format!("{seen:?}"))
}

#[cfg(feature = "probes")]
fn grenade_waits() -> Result<String, String> {
    let (waiting, _) = run(Shelf::Grenade, 2, &SCROLLED, false);
    let (took, _) = run(Shelf::Grenade, 2, &SCROLLED, true);
    sim::probe::check(
        waiting[3] && waiting[4] && !took[3],
        format!("not yet {waiting:?}, taken {took:?}"),
    )
}

/// Scrolled, then a screen comes between: the key still down is up to the sim,
/// and letting it go uses nothing. One with nothing to hold back stays as ever.
#[cfg(feature = "probes")]
fn cancelled_hold() -> Result<String, String> {
    let at = Shelf::Medpac as usize;
    let frame = |held: bool, pressed: bool, count: usize| {
        let mut frame = Frame {
            seconds: 0.01,
            ..Frame::default()
        };
        frame.held[at] = held;
        frame.pressed[at] = pressed;
        frame.counts[at] = count;
        frame
    };
    let mut picking = Picking::default();
    picking.frame(frame(true, true, 2));
    picking.cancel();
    let back = picking.frame(frame(true, false, 2)).set[at];
    let let_go = picking.frame(frame(false, false, 2)).set[at];
    let free = picking.hold.is_none();
    let mut plain = Picking::default();
    plain.frame(frame(true, true, 1));
    plain.cancel();
    sim::probe::check(
        back == Some(false) && let_go.is_none() && free && plain.hold.is_none(),
        format!(
            "back {back:?}, let go {let_go:?}, free {free}, plain {:?}",
            plain.hold
        ),
    )
}

#[cfg(feature = "probes")]
fn label_fades() -> Result<String, String> {
    let mut picking = Picking::default();
    picking.say(LabelOn::Feat, b"x".to_vec(), 10.0);
    let alpha = |now| picking.label(now).map(|(_, _, alpha)| alpha);
    let (full, fading, gone) = (alpha(10.5), alpha(11.25), alpha(11.6));
    sim::probe::check(
        full == Some(1.0) && fading.is_some_and(|alpha| alpha < 1.0) && gone.is_none(),
        format!("{full:?} {fading:?} {gone:?}"),
    )
}
