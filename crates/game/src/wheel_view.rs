//! The power wheel from the player's side. While the player holds the
//! wheel's key (`settings::Action::Wheel`), the wheel is open. It offers
//! everything the character can use that is not on the action bar
//! (`sim::combat::bar::wheel`), on a ring round the middle of the screen
//! (`ui::screens::wheel`).
//!
//! The mouse and the right stick move a pointer from the wheel's centre
//! instead of turning the camera. The entry the pointer points at is
//! highlighted. When the highlighted entry is a kind of item and the
//! character carries two or more different items of that kind, an arc of
//! those items opens beyond it. The mouse picks an arc item by pointing
//! past the ring, and the mouse wheel steps through the arc. On a
//! controller, RB steps on and LB steps back. With no arc open and more than
//! 10 entries, the same steps turn the ring by one entry.
//!
//! Letting go of the key uses the highlighted entry, as a press of a bar
//! slot holding it would (`bar_view::slot_use`). An arc item other than the
//! one the character uses now is picked first (`Action::PickItem`). The
//! wheel closes without using anything when a screen, a conversation or a
//! movie comes up, when the leader changes, or when the character goes
//! down. While the wheel is open in a session of one seat, the game runs at
//! a quarter of its speed (`time_scale`).

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use sim::action::Action;
use sim::combat::Life;
use sim::combat::bar::{self, Content, Rules};
use sim::combat::items::Shelf;
use sim::{SimId, Snapshot};
use ui::screens::wheel::{
    ARC_IN, ARC_OUT, ARC_SHOWN, ARC_STEP, DEAD_IN, DEAD_OUT, REACH, RING_SHOWN, arc_window,
    ring_angle, ring_slot,
};

use crate::bar_view::{BarUse, Means, slot_use, with_bar};
use crate::loot::Outgoing;
use crate::pad::{Actions, Prompts};
use crate::pick_view::{Kind, Picking, TAP};
use crate::settings::Action as Key;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The clock's speed while the wheel is open in a session of one seat
/// (rulebook "Power Wheel Speed").
pub const SLOW: f32 = rules::controls::bar::WHEEL_SLOW;
/// How far the right stick pushed all the way moves the pointer (H, the
/// height of the HUD's frame). It reaches past the ring and stops short of
/// the arc.
const STICK_REACH: f32 = 0.25;
/// How far one count of the mouse moves the pointer (H), at the player's
/// default mouse sensitivity.
const MOUSE_GAIN: f32 = 0.0007;
/// How far past its sector's edge the pointer may go before the highlight
/// leaves an entry (degrees). It is never more than a quarter of a sector.
const KEEP: f32 = 5.0;
/// How long the pointer keeps its place after the right stick springs back
/// to its middle (seconds of real time).
const GRACE: f32 = 0.25;
/// The longest a use waits for the snapshot to show the arc item picked
/// before it goes ahead (seconds).
const PICK_WAIT: f32 = 0.5;

/// The wheel's state, from frame to frame.
#[derive(Resource, Debug, Default)]
pub struct Wheel {
    pub open: bool,
    /// The pointer, in H from the wheel's centre, with y up.
    pointer: Vec2,
    /// The highlighted entry, by its place in the list.
    at: Option<usize>,
    /// The arc item that letting go uses, by its place in the arc.
    arc_at: Option<usize>,
    /// The list entry the ring shows in its top slot.
    first: usize,
    /// The pointer is in the arc.
    in_arc: bool,
    /// The body the wheel opened for.
    body: Option<SimId>,
    /// Something closed the wheel while its key was held. The wheel stays
    /// shut until the key is let go.
    cancelled: bool,
    /// The right stick set the pointer last.
    by_stick: bool,
    /// How long the right stick has rested in its middle (seconds).
    rested: f32,
    /// How far the mouse wheel turned short of a notch.
    spare: f32,
    /// A use on its way after the key was let go.
    due: Option<Due>,
    /// What a run's commands do with the wheel (`script`).
    scripted: Scripted,
}

/// What a run's commands do with the wheel: a headless run reads no device.
#[derive(Debug, Default)]
struct Scripted {
    held: bool,
    /// Where the commands put the pointer (H).
    point: Option<Vec2>,
    steps: i32,
}

/// One frame of what the wheel reads (`Wheel::frame`).
pub struct Frame<'a> {
    /// The wheel's key is held.
    pub held: bool,
    pub screen: bool,
    pub talk: bool,
    pub movie: bool,
    /// The body the seat plays, and whether it is alive.
    pub body: Option<(SimId, bool)>,
    /// How far the mouse moved the pointer this frame (H, y up).
    pub mouse: Vec2,
    /// The right stick, rested, from 0 to 1 each way (y up).
    pub stick: Vec2,
    /// The mouse wheel's notches, LB and RB: on is positive.
    pub steps: i32,
    /// For each list entry, how many items its arc has and which of them
    /// the character uses now. An entry with no arc has (0, 0).
    pub arcs: &'a [(usize, usize)],
    /// Seconds of real time since the last frame.
    pub seconds: f32,
}

/// What letting go uses: the list entry and, with an arc open, the arc
/// item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    pub at: usize,
    pub arc_at: Option<usize>,
}

/// The pointer's bearing, in degrees clockwise from straight up.
fn bearing(pointer: Vec2) -> f32 {
    pointer.x.atan2(pointer.y).to_degrees().rem_euclid(360.0)
}

/// How far bearing `a` is from bearing `b`, from -180 to 180 degrees.
fn apart(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

/// The ring slot a pointer selects on a ring of `entries` slots. None
/// inside the dead zone, and none on an empty ring.
pub fn entry_at(pointer: Vec2, entries: usize) -> Option<usize> {
    if entries == 0 || pointer.length() < DEAD_IN {
        return None;
    }
    let sector = 360.0 / entries as f32;
    Some((bearing(pointer) / sector).round() as usize % entries)
}

/// The ring slot the pointer selects when `was` was selected before. The
/// slot stays selected while the pointer is long enough and within half a
/// sector, plus a margin, of the slot, so the highlight does not flicker
/// between two neighbours.
fn keep(pointer: Vec2, entries: usize, was: Option<usize>) -> Option<usize> {
    if let Some(was) = was.filter(|_| entries > 0 && pointer.length() >= DEAD_OUT) {
        let sector = 360.0 / entries as f32;
        let margin = KEEP.min(sector / 4.0);
        if apart(bearing(pointer), ring_angle(was, entries)).abs() <= sector / 2.0 + margin {
            return Some(was);
        }
    }
    entry_at(pointer, entries)
}

/// The place, among the `shown` arc items beyond the entry at `entry`
/// degrees, that the pointer's bearing `towards` selects. The item at
/// `was` stays selected within a quarter step past its own half step. A
/// bearing past either end of the arc selects the end item.
fn arc_place(towards: f32, entry: f32, shown: usize, was: Option<usize>) -> usize {
    let middle = (shown as f32 - 1.0) / 2.0;
    let place = apart(towards, entry) / ARC_STEP + middle;
    if let Some(was) = was
        && (place - was as f32).abs() <= 0.75
    {
        return was;
    }
    place.round().clamp(0.0, shown as f32 - 1.0) as usize
}

/// The clock's scale: `SLOW` while the wheel is open in a session of one
/// seat, else full speed.
pub fn time_scale(open: bool, seats: u8) -> f32 {
    if open && seats == 1 { SLOW } else { 1.0 }
}

impl Wheel {
    /// An open wheel, for the probes of other modules.
    #[cfg(feature = "probes")]
    pub(crate) fn opened() -> Wheel {
        Wheel {
            open: true,
            ..Wheel::default()
        }
    }

    /// A command's request: `powerwheel hold`, `powerwheel up`,
    /// `powerwheel point X Y` (the pointer in H, y up) or
    /// `powerwheel step N`.
    pub(crate) fn script(&mut self, words: &[&str]) -> bool {
        let number = |word: &str| word.parse::<f32>().ok();
        match words {
            ["powerwheel", "hold"] => self.scripted.held = true,
            ["powerwheel", "up"] => self.scripted.held = false,
            ["powerwheel", "point", x, y] => match (number(x), number(y)) {
                (Some(x), Some(y)) => self.scripted.point = Some(Vec2::new(x, y)),
                _ => return false,
            },
            ["powerwheel", "step", steps] => match steps.parse::<i32>() {
                Ok(steps) => self.scripted.steps += steps,
                Err(_) => return false,
            },
            _ => return false,
        }
        true
    }

    /// Takes one frame. Returns what letting go uses, on the frame the key
    /// is let go with something highlighted.
    pub fn frame(&mut self, frame: &Frame) -> Option<Release> {
        if !frame.held {
            self.cancelled = false;
        }
        let alive = frame.body.filter(|(_, alive)| *alive).map(|(id, _)| id);
        let blocked = frame.screen || frame.talk || frame.movie || alive.is_none();
        if blocked {
            self.due = None;
        }
        if self.open && (blocked || alive != self.body) {
            self.open = false;
            self.cancelled = frame.held;
            return None;
        }
        if self.open && !frame.held {
            self.open = false;
            let arc = self.arc_items(frame.arcs) >= 2;
            return self.at.map(|at| Release {
                at,
                arc_at: self.arc_at.filter(|_| arc),
            });
        }
        if !self.open && frame.held && !self.cancelled && !blocked {
            *self = Wheel {
                open: true,
                body: alive,
                due: self.due.take(),
                scripted: std::mem::take(&mut self.scripted),
                ..Wheel::default()
            };
        }
        if self.open {
            self.steer(frame);
        }
        None
    }

    /// How many items the highlighted entry's arc has.
    fn arc_items(&self, arcs: &[(usize, usize)]) -> usize {
        self.at
            .and_then(|at| arcs.get(at))
            .map_or(0, |(items, _)| *items)
    }

    /// The highlighted entry becomes `at`. Its arc, if it has one, starts
    /// on the item the character uses now.
    fn highlight(&mut self, at: Option<usize>, arcs: &[(usize, usize)]) {
        self.at = at;
        self.arc_at = at
            .and_then(|at| arcs.get(at))
            .filter(|(items, _)| *items >= 2)
            .map(|(_, chosen)| *chosen);
    }

    /// Moves the pointer, then the highlight and the arc's choice, then
    /// takes the steps.
    fn steer(&mut self, frame: &Frame) {
        let before = self.pointer;
        if frame.mouse != Vec2::ZERO {
            self.pointer = (self.pointer + frame.mouse).clamp_length_max(REACH);
            self.by_stick = false;
        }
        if frame.stick != Vec2::ZERO {
            self.pointer = frame.stick * STICK_REACH;
            self.by_stick = true;
            self.rested = 0.0;
        } else if self.by_stick {
            self.rested += frame.seconds;
            if self.rested >= GRACE {
                self.pointer = Vec2::ZERO;
                self.by_stick = false;
            }
        }
        let entries = frame.arcs.len();
        if self.at.is_some_and(|at| at >= entries) {
            self.highlight(None, frame.arcs);
        }
        if entries == 0 {
            return;
        }
        let shown = entries.min(RING_SHOWN);
        let items = self.arc_items(frame.arcs);
        let length = self.pointer.length();
        self.in_arc = items >= 2
            && if self.in_arc {
                length >= ARC_OUT
            } else {
                length > ARC_IN
            };
        let slot = self.at.and_then(|at| ring_slot(at, self.first, entries));
        if !self.in_arc {
            let picked = keep(self.pointer, shown, slot).map(|slot| (self.first + slot) % entries);
            if picked != self.at {
                self.highlight(picked, frame.arcs);
            }
        } else if let Some(slot) = slot.filter(|_| self.pointer != before) {
            // The pointer picks among the arc items in view only on a frame
            // it moves, so a step taken with the pointer still is kept.
            let from = arc_window(self.arc_at.unwrap_or(0), items);
            let in_view = items.min(ARC_SHOWN);
            let was = self
                .arc_at
                .and_then(|at| at.checked_sub(from))
                .filter(|place| *place < in_view);
            let entry = ring_angle(slot, shown);
            self.arc_at = Some(from + arc_place(bearing(self.pointer), entry, in_view, was));
        }
        if frame.steps == 0 {
            return;
        }
        if let Some(at) = self.arc_at.filter(|_| items >= 2) {
            self.arc_at = Some((at as i32 + frame.steps).rem_euclid(items as i32) as usize);
        } else if entries > RING_SHOWN {
            self.first = (self.first as i32 + frame.steps).rem_euclid(entries as i32) as usize;
            let picked = slot.map(|slot| (self.first + slot) % entries);
            self.highlight(picked, frame.arcs);
        }
    }
}

/// A use on its way after the wheel's key was let go: the pick of an arc
/// item to send first, then a short press of what the entry holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    content: Content,
    /// The powers the force button can cast.
    choices: Vec<u16>,
    usable: bool,
    hand: u8,
    target: Option<u32>,
    /// The arc item to pick before the use: its kind and its row.
    pick: Option<(Shelf, u16)>,
    /// The pick went out.
    sent: bool,
    /// How long the use has waited for the pick (seconds).
    waited: f32,
    /// How long the press has left (seconds), once it has started.
    tap: Option<f32>,
}

impl Due {
    pub fn new(
        content: Content,
        means: &Means,
        target: Option<u32>,
        pick: Option<(Shelf, u16)>,
    ) -> Due {
        Due {
            content,
            choices: means.choices.clone(),
            usable: crate::bar_view::usable(content, means),
            hand: means.fighter.hand,
            target,
            pick,
            sent: false,
            waited: 0.0,
            tap: None,
        }
    }

    /// One frame of the use, `seconds` after the last. `confirmed` says the
    /// snapshot shows the pick. Returns the actions to send this frame and
    /// what the bar holds down.
    ///
    /// The pick goes out on the first frame. The use waits for the snapshot
    /// to show the pick, at most `PICK_WAIT`, and then holds the press for
    /// `TAP`, so a sim step cannot miss it. The press's own actions go out
    /// on its first frame.
    pub fn step(&mut self, confirmed: bool, seconds: f32) -> (Vec<Action>, BarUse) {
        let mut sent = Vec::new();
        if !std::mem::replace(&mut self.sent, true)
            && let Some((_, row)) = self.pick
        {
            sent.push(Action::PickItem { row });
        }
        let waiting = self.pick.is_some() && !confirmed && self.waited < PICK_WAIT;
        if self.tap.is_none() && waiting {
            self.waited += seconds;
            return (sent, BarUse::default());
        }
        let pressed = self.tap.is_none();
        *self.tap.get_or_insert(TAP) -= seconds;
        let (choices, usable) = (&self.choices, self.usable);
        let mut used = slot_use(
            self.content,
            choices,
            usable,
            pressed,
            true,
            self.hand,
            self.target,
        );
        sent.append(&mut used.actions);
        (sent, used)
    }

    /// The press is over.
    pub fn done(&self) -> bool {
        self.tap.is_some_and(|left| left <= 0.0)
    }
}

/// What the wheel offers the character now: the list, the items of each
/// kind, and which of them the character uses now.
struct Listed {
    list: Vec<Content>,
    kinds: [Vec<Kind>; Shelf::ALL.len()],
    chosen: [Option<usize>; Shelf::ALL.len()],
}

impl Listed {
    fn new(
        snapshot: &Snapshot,
        (means, rules): (&Means, &Rules),
        library: Option<&std::sync::Arc<sim::script::Library>>,
        picking: Option<&Picking>,
    ) -> Listed {
        let (sheet, fighter) = (&means.sheet, means.fighter);
        let kinds = crate::pick_view::kinds(snapshot, means.me, sheet, fighter, library);
        let chosen = Shelf::ALL.map(|shelf| {
            let pending = picking.and_then(|picking| picking.pending(shelf));
            crate::pick_view::chosen(&kinds, sheet, fighter, shelf, pending, snapshot.time_ms)
        });
        Listed {
            list: bar::wheel(sheet, rules, fighter.hand),
            kinds,
            chosen,
        }
    }

    /// Each entry's arc (`Frame::arcs`).
    fn arcs(&self) -> Vec<(usize, usize)> {
        self.list
            .iter()
            .map(|content| match content {
                Content::Item(shelf) => {
                    let at = *shelf as usize;
                    (self.kinds[at].len(), self.chosen[at].unwrap_or(0))
                }
                _ => (0, 0),
            })
            .collect()
    }

    /// The use `release` asks for: the entry's content, and the pick of an
    /// arc item other than the one the character uses now.
    fn release(&self, release: Release) -> Option<(Content, Option<(Shelf, u16)>)> {
        let content = *self.list.get(release.at)?;
        let pick = match (content, release.arc_at) {
            (Content::Item(shelf), Some(item)) if self.chosen[shelf as usize] != Some(item) => {
                let kind = self.kinds[shelf as usize].get(item)?;
                Some((shelf, kind.row))
            }
            _ => None,
        };
        Some((content, pick))
    }
}

/// The wheel has read this frame's keys and pointer.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Steered;

/// The wheel's use is in `BarUse`, which the commands come after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Released;

pub struct WheelViewPlugin;

impl Plugin for WheelViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wheel>()
            .add_systems(
                PreUpdate,
                (
                    steer
                        .in_set(Steered)
                        .after(crate::pad::Sampled)
                        .before(crate::pick_view::Picked),
                    let_go.in_set(Released).after(crate::bar_view::Used),
                ),
            )
            .add_systems(Update, show.before(crate::ui_view::Stepped));
    }
}

/// The mouse wheel's whole notches this frame (`pick_view::turned`), with
/// what was left short of a notch before, which `spare` keeps. Down is
/// on, as on the target's actions.
fn notches(wheel: &mut MessageReader<MouseWheel>, spare: &mut f32) -> i32 {
    *spare += crate::pick_view::turned(wheel);
    let notches = spare.trunc();
    *spare -= notches;
    -(notches as i32)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn steer(
    actions: Option<Res<Actions>>,
    (motion, mut scroll, cursors, feel): (
        Option<Res<AccumulatedMouseMotion>>,
        MessageReader<MouseWheel>,
        Query<&CursorOptions, With<PrimaryWindow>>,
        Option<Res<crate::settings::MouseFeel>>,
    ),
    (ui, replies, showing): (
        Option<Res<Ui>>,
        Option<Res<crate::dialogue_view::ReplyInput>>,
        Option<Res<crate::movie_view::Showing>>,
    ),
    (presented, own, swap): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
    ),
    (picking, targeted): (
        Option<Res<Picking>>,
        Option<Res<crate::target_view::Targeted>>,
    ),
    mut wheel: ResMut<Wheel>,
    time: Res<Time<Real>>,
) {
    let wheel = &mut *wheel;
    let seen = presented.as_ref().zip(own.as_ref()).zip(swap.as_ref());
    let snapshot = seen.map(|((presented, _), _)| presented.0.current());
    let mut listed = None;
    let body = seen.and_then(|((presented, own), swap)| {
        let snapshot = presented.0.current();
        let (body, _) = crate::cast::body_sheet(snapshot, own.0, &swap.level)?;
        let alive = body.fighter.as_ref()?.life == Life::Alive;
        listed = with_bar(snapshot, own.0, &swap.level, |means, rules| {
            let library = swap.level.scripts();
            Listed::new(snapshot, (means, rules), library, picking.as_deref())
        });
        Some((body.id, alive))
    });
    let arcs = listed.as_ref().map_or_else(Vec::new, Listed::arcs);
    let grabbed = cursors
        .iter()
        .any(|cursor| cursor.grab_mode != CursorGrabMode::None);
    let gain = MOUSE_GAIN * feel.map_or(1.0, |feel| feel.scale);
    let mut mouse = motion.filter(|_| grabbed).map_or(Vec2::ZERO, |motion| {
        Vec2::new(motion.delta.x, -motion.delta.y) * gain
    });
    // A point the commands asked for waits until the wheel is open, since
    // opening puts the pointer back at the centre.
    if wheel.open
        && let Some(point) = wheel.scripted.point.take()
    {
        mouse = point - wheel.pointer;
    }
    let steps = notches(&mut scroll, &mut wheel.spare)
        + actions
            .as_ref()
            .map_or(0, |actions| i32::from(actions.wheel_step))
        + std::mem::take(&mut wheel.scripted.steps);
    let frame = Frame {
        held: actions
            .as_ref()
            .is_some_and(|actions| actions.held(Key::Wheel))
            || wheel.scripted.held,
        screen: ui.is_some_and(|ui| ui.stack.is_open()),
        talk: replies.is_some_and(|replies| replies.active),
        movie: showing.is_some_and(|showing| showing.0),
        body,
        mouse,
        stick: actions.as_ref().map_or(Vec2::ZERO, |actions| actions.aim),
        steps,
        arcs: &arcs,
        seconds: time.delta_secs(),
    };
    if !wheel.open {
        wheel.spare = 0.0;
    }
    let Some(release) = wheel.frame(&frame) else {
        return;
    };
    let (Some(snapshot), Some(((_, own), swap)), Some(listed)) = (snapshot, seen, listed) else {
        return;
    };
    let foe = crate::bar_view::foe(targeted.as_deref(), snapshot);
    wheel.due = with_bar(snapshot, own.0, &swap.level, |means, rules| {
        let (content, pick) = listed.release(release)?;
        let target = crate::bar_view::cast_target(rules, content, foe, means.me.0);
        Some(Due::new(content, means, target, pick))
    })
    .flatten();
}

/// Plays out the wheel's use: sends its pick and its actions, and holds its
/// buttons in `BarUse` beside the bar's.
#[allow(clippy::type_complexity)]
fn let_go(
    mut wheel: ResMut<Wheel>,
    mut bar_use: ResMut<BarUse>,
    mut outgoing: Option<ResMut<Outgoing>>,
    (presented, own, swap): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
    ),
    time: Res<Time<Real>>,
) {
    let Some(due) = wheel.due.as_mut() else {
        return;
    };
    let picked = presented
        .as_ref()
        .zip(own.as_ref())
        .zip(swap.as_ref())
        .and_then(|((presented, own), swap)| {
            let snapshot = presented.0.current();
            let (_, sheet) = crate::cast::body_sheet(snapshot, own.0, &swap.level)?;
            let (shelf, _) = due.pick?;
            sheet.picks.item(shelf)
        });
    let confirmed = due.pick.is_some_and(|(_, row)| picked == Some(row));
    let (sent, used) = due.step(confirmed, time.delta_secs());
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(sent);
    }
    bar_use.force = bar_use.force.or(used.force);
    bar_use.buttons |= used.buttons;
    if due.done() {
        wheel.due = None;
    }
}

/// What the wheel shows of each list entry, and of the arc of the
/// highlighted entry.
fn entries(
    ui: &mut Ui,
    wheel: &Wheel,
    listed: &Listed,
    (means, snapshot): (&Means, &Snapshot),
    library: Option<&sim::script::Library>,
) -> (Vec<ui::vm::WheelEntry>, Vec<ui::vm::WheelEntry>) {
    let chosen: crate::hud::Chosen = Shelf::ALL.map(|shelf| {
        let at = shelf as usize;
        listed.chosen[at].and_then(|chosen| listed.kinds[at].get(chosen).cloned())
    });
    let kits = library.map_or(0, |library| {
        sim::traps::party_kits(&snapshot.goods, library.items())
    });
    let shown = listed
        .list
        .iter()
        .map(|content| crate::hud::bar_entry(ui, *content, means, &chosen, kits))
        .collect();
    let arc_of = wheel
        .at
        .and_then(|at| listed.list.get(at))
        .and_then(|content| match content {
            Content::Item(shelf) => Some(*shelf as usize),
            _ => None,
        })
        .filter(|at| listed.kinds[*at].len() >= 2);
    let arc = arc_of.map_or_else(Vec::new, |at| {
        listed.kinds[at]
            .iter()
            .map(|kind| {
                // An item with no template shows a stock item of its kind.
                let templates = kind.template.as_deref().into_iter();
                let stock = crate::hud::SHELF_ICONS[at].iter().copied();
                let icon = ui
                    .assets
                    .as_mut()
                    .and_then(|assets| crate::hud::item_icon(assets, templates.chain(stock)));
                ui::vm::WheelEntry {
                    icon: icon.unwrap_or(String::new()),
                    name: crate::pick_view::item_name(ui, kind).unwrap_or(Vec::new()),
                    count: Some(kind.left),
                    ..ui::vm::WheelEntry::default()
                }
            })
            .collect()
    });
    (shown, arc)
}

/// Fills the view-model's wheel while the wheel is open.
#[allow(clippy::type_complexity)]
fn show(
    wheel: Res<Wheel>,
    ui: Option<ResMut<Ui>>,
    (presented, own, swap, picking): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
        Option<Res<Picking>>,
    ),
    prompts: Prompts,
    (actions, mut facts): (Option<Res<Actions>>, ResMut<console::DumpFacts>),
) {
    let Some(mut ui) = ui else {
        return;
    };
    if facts.wanted() {
        // Each entry's icon and name, the highlight and the arc, for a
        // run's dump.
        let named = |entries: &[ui::vm::WheelEntry]| {
            let named = entries.iter().map(|entry| {
                let name = String::from_utf8_lossy(&entry.name).into_owned();
                (entry.icon.clone(), name, entry.count, entry.dim)
            });
            named.collect::<Vec<_>>()
        };
        let shown = ui.vm.wheel.as_ref().map(|view| {
            serde_json::json!({
                "entries": named(&view.entries), "first": view.first, "at": view.at,
                "arc": named(&view.arc), "arc_at": view.arc_at,
            })
        });
        facts
            .0
            .insert("wheel".into(), shown.unwrap_or(serde_json::Value::Null));
    }
    let ui = &mut *ui;
    let seen = presented.zip(own).zip(swap).filter(|_| wheel.open);
    let Some(((presented, own), swap)) = seen else {
        ui.vm.wheel = None;
        return;
    };
    let snapshot = presented.0.current();
    let library = swap.level.scripts();
    let shown = with_bar(snapshot, own.0, &swap.level, |means, rules| {
        let listed = Listed::new(snapshot, (means, rules), library, picking.as_deref());
        let library = library.map(|library| &**library);
        entries(ui, &wheel, &listed, (means, snapshot), library)
    });
    ui.vm.wheel = shown.map(|(entries, arc)| ui::vm::WheelView {
        entries,
        first: wheel.first,
        at: wheel.at,
        arc_at: wheel.arc_at.filter(|_| !arc.is_empty()),
        arc,
        key: prompts
            .bound_label(Key::Wheel)
            .unwrap_or(ui::vm::KeyLabel::default()),
        bumpers: crate::pad::wheel_badges(actions.as_deref()),
    });
}

/// The wheel's rules, no window (`kjk sim probes`, group "power_wheel").
#[cfg(feature = "probes")]
pub mod probes;
