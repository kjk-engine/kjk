//! The power wheel from the player's side. While the player holds the
//! wheel's key (`settings::Action::Wheel`), the wheel is open. It offers
//! everything the character can use that is not on the action bar
//! (`sim::combat::bar::wheel`), on a ring round the middle of the screen
//! (`ui::screens::wheel`). An item kind on the bar stays on the wheel while
//! the character carries two or more different items of that kind, and its
//! entry shows a stack of frames behind it.
//!
//! The mouse and the right stick move a pointer from the wheel's centre
//! instead of turning the camera. The entry the pointer points at is
//! highlighted. The entries stand in their groups, and the ring grows to
//! show up to 28 of them at once (`ui::screens::wheel::Ring`). When the
//! highlighted entry is a kind of item and the character carries two or
//! more different items of that kind, an arc of those items opens straight
//! outside it the moment it is highlighted. Moving the pointer further out
//! keeps the entry highlighted, and the arc's choice starts on the item the
//! character uses now.
//!
//! The mouse picks the arc item its pointer points at. The right stick
//! moves the pointer in proportion to how far it is pushed. A push all the
//! way moves the pointer out to the arc, and a push of less than about 85%
//! keeps it on the ring. When the stick reaches the arc, the choice stays on
//! the item chosen so far, so a player who flicks the stick towards an item
//! kind and lets go uses the item the character uses now. Turning the stick
//! along the rim from there moves the choice by one item for each arc item's
//! angle turned, and it stops at the arc's first and last items.
//!
//! The mouse wheel also steps through the arc. On a controller, RB held is
//! the wheel's key, LB steps on by one item, and nothing steps back. With no arc open and more
//! than 28 entries, the same steps turn the ring by one entry.
//!
//! The pointer starts at the wheel's centre, and a small dot shows where it
//! is. While the pointer is in the centre (closer than
//! `ui::screens::wheel::DEAD_IN`, or with the right stick pushed less than
//! `STICK_CENTRE`), nothing is highlighted. Moving the pointer back to the
//! centre clears the highlight, so letting go there uses nothing.
//!
//! Letting go of the key uses the highlighted entry, as a press of a bar
//! slot holding it would (`bar_view::slot_use`). Each use joins the end of
//! a queue, as KOTOR's actions do. The first use in the queue waits until
//! the character has a free hand for it (`ready`), and the next one starts
//! only after it. So a player who picks Force Valor, Force Armor and Force
//! Speed while the character is still casting Force Valor sees the three
//! powers cast one after another. The action bar's slots join the same
//! queue while it has uses in it or while the character is busy
//! (`bar_view`). An arc item other than the
//! one the character uses now is picked first (`Action::PickItem`). Letting
//! go on grenades readies a grenade, or puts it away when one is readied, and
//! a grenade picked from the arc is readied. Nothing is thrown. The
//! wheel closes without using anything when a screen, a conversation or a
//! movie comes up, when the leader changes, or when the character goes
//! down. While the wheel is open in a session of one seat, the game runs at
//! a quarter of its speed (`time_scale`).

use std::collections::VecDeque;

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use sim::action::Action;
use sim::combat::bar::{self, Content, Rules};
use sim::combat::items::Shelf;
use sim::combat::{Fighter, Life};
use sim::{SimId, Snapshot};
use ui::screens::wheel::{
    ARC_SHOWN, DEAD_IN, DEAD_OUT, RING_SHOWN, Ring, apart, arc_window, ring_slot,
};

use crate::bar_view::{BarUse, Means, slot_use, with_bar};
use crate::loot::Outgoing;
use crate::pad::Actions;
use crate::pick_view::{Kind, Picking};
use crate::settings::Action as Key;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The clock's speed while the wheel is open in a session of one seat
/// (rulebook "Power Wheel Speed").
pub const SLOW: f32 = rules::controls::bar::WHEEL_SLOW;
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
/// A push of the right stick shorter than this (from 0 to 1, past the
/// pad's own dead middle) leaves the pointer in the wheel's centre, where
/// nothing is highlighted.
pub const STICK_CENTRE: f32 = 0.3;

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
    /// Where the right stick's turns along the arc count from. It is the
    /// pointer's bearing when the stick reached the arc, or when LB last
    /// stepped (degrees), and the arc item chosen then. None while the
    /// pointer is out of the arc or the mouse moved it last.
    anchor: Option<(f32, usize)>,
    /// How long the right stick has rested in its middle (seconds).
    rested: f32,
    /// How far the mouse wheel turned short of a notch.
    spare: f32,
    /// The uses on their way after the key was let go, and the action bar's
    /// uses that wait behind them, first in line first.
    queue: VecDeque<Due>,
    /// What a run's commands do with the wheel (`script`).
    scripted: Scripted,
}

/// What a run's commands do with the wheel. A headless run reads no
/// device, so its commands stand in for the key, the pointer and the steps.
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
    /// The mouse wheel's notches and LB's presses. A step on is positive.
    pub steps: i32,
    /// For each list entry, how many items its arc has and which of them
    /// the character uses now. An entry with no arc has (0, 0).
    pub arcs: &'a [(usize, usize)],
    /// Each list entry's group (`sim::combat::bar::Group`).
    pub groups: &'a [u8],
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

/// The ring slot a pointer selects on `ring`: the slot nearest to the
/// pointer's bearing. None inside the dead zone, and none on an empty ring.
pub fn entry_at(pointer: Vec2, ring: &Ring) -> Option<usize> {
    if pointer.length() < DEAD_IN {
        return None;
    }
    ring.nearest(bearing(pointer))
}

/// The ring slot the pointer selects when `was` was selected before. The
/// slot stays selected while the pointer is long enough and no further
/// than a margin past the halfway line between the slot and the slot
/// nearest to the pointer, so the highlight does not flicker between two
/// neighbours. The margin is at most a quarter of the way between them.
fn keep(pointer: Vec2, ring: &Ring, was: Option<usize>) -> Option<usize> {
    let towards = bearing(pointer);
    let was = was.filter(|was| *was < ring.angles.len() && pointer.length() >= DEAD_OUT);
    if let (Some(was), Some(near)) = (was, ring.nearest(towards)) {
        let edge = apart(ring.between(was, near), ring.angles[was]).abs();
        if apart(towards, ring.angles[was]).abs() <= edge + KEEP.min(edge / 2.0) {
            return Some(was);
        }
    }
    entry_at(pointer, ring)
}

/// The place, among the `shown` arc items `step` degrees apart beyond the
/// entry at `entry` degrees, that the pointer's bearing `towards` selects.
/// The item at `was` stays selected within a quarter step past its own
/// half step. A bearing past either end of the arc selects the end item.
fn arc_place(towards: f32, (entry, step): (f32, f32), shown: usize, was: Option<usize>) -> usize {
    let middle = (shown as f32 - 1.0) / 2.0;
    let place = apart(towards, entry) / step + middle;
    if let Some(was) = was
        && (place - was as f32).abs() <= 0.75
    {
        return was;
    }
    place.round().clamp(0.0, shown as f32 - 1.0) as usize
}

/// The arc item, of `items`, that the right stick chooses at bearing
/// `towards`. The stick counts from its `anchor`, the bearing and the item
/// chosen there. Each `step` degrees turned from the anchor's bearing moves
/// the choice by one item, and a turn past either end of the arc chooses
/// the end item. The item at `was` stays chosen while the turn is within
/// three quarters of a step of it, so a stick that trembles does not flip
/// between two items.
fn arc_turn(
    towards: f32,
    (from, item): (f32, usize),
    step: f32,
    items: usize,
    was: usize,
) -> usize {
    let place = item as f32 + apart(towards, from) / step;
    if (place - was as f32).abs() <= 0.75 {
        return was;
    }
    place.round().clamp(0.0, items as f32 - 1.0) as usize
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

    /// Puts `due` at the end of the queue of uses.
    pub(crate) fn queue(&mut self, due: Due) {
        self.queue.push_back(due);
    }

    /// Whether uses wait in the queue.
    pub(crate) fn queued(&self) -> bool {
        !self.queue.is_empty()
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
            self.queue.clear();
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
                queue: std::mem::take(&mut self.queue),
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
        let entries = frame.arcs.len();
        let ring = Ring::shown(&frame.groups[..entries.min(frame.groups.len())], self.first);
        if frame.mouse != Vec2::ZERO {
            self.pointer = (self.pointer + frame.mouse).clamp_length_max(ring.reach());
            self.by_stick = false;
            self.anchor = None;
        }
        if frame.stick != Vec2::ZERO {
            let push = frame.stick.clamp_length_max(1.0);
            self.pointer = if push.length() < STICK_CENTRE {
                Vec2::ZERO
            } else {
                push * ring.arc()
            };
            self.by_stick = true;
            self.rested = 0.0;
        } else if self.by_stick {
            self.rested += frame.seconds;
            if self.rested >= GRACE {
                self.pointer = Vec2::ZERO;
                self.by_stick = false;
            }
        }
        if self.at.is_some_and(|at| at >= entries) {
            self.highlight(None, frame.arcs);
        }
        if entries == 0 {
            return;
        }
        let items = self.arc_items(frame.arcs);
        let length = self.pointer.length();
        self.in_arc = items >= 2
            && if self.in_arc {
                length >= ring.arc_out()
            } else {
                length > ring.arc_in()
            };
        let slot = self.at.and_then(|at| ring_slot(at, self.first, entries));
        let moved = self.pointer != before;
        if !self.in_arc {
            self.anchor = None;
            let picked = keep(self.pointer, &ring, slot).map(|slot| (self.first + slot) % entries);
            if picked != self.at {
                self.highlight(picked, frame.arcs);
            }
        } else if slot.is_some() && self.by_stick {
            let towards = bearing(self.pointer);
            match self.anchor {
                // The stick has just reached the arc. The choice stays on
                // the item chosen so far, and the stick counts from here.
                None => self.anchor = self.arc_at.map(|at| (towards, at)),
                Some(anchor) if moved => {
                    let step = ring.arc_step();
                    self.arc_at = self
                        .arc_at
                        .map(|was| arc_turn(towards, anchor, step, items, was));
                }
                Some(_) => {}
            }
        } else if let Some(slot) = slot.filter(|_| moved) {
            // The pointer picks among the arc items in view only on a frame
            // it moves, so a step taken with the pointer still is kept.
            let from = arc_window(self.arc_at.unwrap_or(0), items);
            let in_view = items.min(ARC_SHOWN);
            let was = self
                .arc_at
                .and_then(|at| at.checked_sub(from))
                .filter(|place| *place < in_view);
            let entry = (ring.angles[slot], ring.arc_step());
            self.arc_at = Some(from + arc_place(bearing(self.pointer), entry, in_view, was));
        }
        if frame.steps == 0 {
            return;
        }
        if let Some(at) = self.arc_at.filter(|_| items >= 2) {
            let at = (at as i32 + frame.steps).rem_euclid(items as i32) as usize;
            self.arc_at = Some(at);
            // The stick's next turn counts on from the stepped item.
            if self.anchor.is_some() {
                self.anchor = Some((bearing(self.pointer), at));
            }
        } else if entries > RING_SHOWN {
            self.first = (self.first as i32 + frame.steps).rem_euclid(entries as i32) as usize;
            let picked = slot.map(|slot| (self.first + slot) % entries);
            self.highlight(picked, frame.arcs);
        }
    }
}

/// Whether the character can start `content` now, so a use in the queue
/// may go ahead. A Force power, an item other than a grenade, and a mine
/// wait until the character has a free hand (`Fighter::hand_free`): no
/// power cast is still under way, and no item is in use. A feat attack,
/// stealth mode and the grenades go ahead at once.
pub(crate) fn ready(content: Content, fighter: &Fighter, now_ms: u64) -> bool {
    match content {
        Content::Power(_) | Content::Mine => fighter.hand_free(now_ms, false),
        Content::Item(shelf) if shelf != Shelf::Grenade => fighter.hand_free(now_ms, false),
        Content::Item(_) | Content::Feat(_) | Content::Stealth => true,
    }
}

/// What a use in the queue sees of the sim on a frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Seen {
    /// The snapshot shows the arc item picked.
    pub confirmed: bool,
    /// The character can start the use now (`ready`).
    pub ready: bool,
    /// The snapshot's time (sim ms).
    pub now_ms: u64,
}

/// A use on its way, from the wheel or from an action bar slot. It first
/// sends the pick of an arc item, if one was chosen. It then waits until
/// the character can start it, and makes a short press of what the entry
/// contains.
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
    /// The snapshot's time when the press went down (sim ms).
    down: Option<u64>,
    /// The snapshot's time when the press went up again (sim ms).
    up: Option<u64>,
    /// The use is over.
    over: bool,
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
            down: None,
            up: None,
            over: false,
        }
    }

    /// What the use is of.
    pub fn content(&self) -> Content {
        self.content
    }

    /// One frame of the use, `seconds` after the last, with what the
    /// snapshot shows (`Seen`). Returns the actions to send this frame and
    /// what the bar holds down.
    ///
    /// The pick goes out on the first frame. The use waits for the snapshot
    /// to show the pick, at most `PICK_WAIT`, and then until the character
    /// can start it. The press holds its buttons down until a sim step has
    /// seen them, which the snapshot shows by moving on in time. The press's
    /// own actions go out on its first frame. The buttons then stay up until
    /// a step has seen them up, so the next use in the queue makes a new
    /// press.
    pub fn step(&mut self, seen: Seen, seconds: f32) -> (Vec<Action>, BarUse) {
        let mut sent = Vec::new();
        if !std::mem::replace(&mut self.sent, true)
            && let Some((_, row)) = self.pick
        {
            sent.push(Action::PickItem { row });
        }
        // A grenade picked from the arc is readied by its pick (the sim's
        // `ready_grenades`). A press would put it away again.
        if matches!(self.pick, Some((Shelf::Grenade, _))) {
            self.over = true;
            return (sent, BarUse::default());
        }
        let pressed = self.down.is_none();
        if pressed {
            let waiting = self.pick.is_some() && !seen.confirmed && self.waited < PICK_WAIT;
            if waiting {
                self.waited += seconds;
                return (sent, BarUse::default());
            }
            if !seen.ready {
                return (sent, BarUse::default());
            }
            self.down = Some(seen.now_ms);
        }
        if self.up.is_none() && self.down.is_some_and(|down| seen.now_ms <= down) {
            let mut used = slot_use(
                self.content,
                &self.choices,
                self.usable,
                pressed,
                true,
                self.hand,
                self.target,
            );
            sent.append(&mut used.actions);
            return (sent, used);
        }
        let up = *self.up.get_or_insert(seen.now_ms);
        self.over = seen.now_ms > up;
        (sent, BarUse::default())
    }

    /// The use is over.
    pub fn done(&self) -> bool {
        self.over
    }
}

/// What the wheel offers the character now: the list, the items of each
/// kind, and which of them the character uses now.
struct Listed {
    list: Vec<Content>,
    /// Each entry's group (`Rules::group`).
    groups: Vec<u8>,
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
        // An item kind on the bar stays on the wheel while the character
        // carries two or more different items of it, so its arc still
        // reaches them.
        let several = |shelf: Shelf| kinds[shelf as usize].len() >= 2;
        let list = bar::wheel(sheet, rules, fighter.hand, &several);
        Listed {
            groups: list
                .iter()
                .map(|content| rules.group(*content) as u8)
                .collect(),
            list,
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
    let held = actions
        .as_ref()
        .is_some_and(|actions| actions.held(Key::Wheel))
        || wheel.scripted.held;
    if !held && !wheel.open && wheel.queue.is_empty() {
        // The wheel is shut and stays shut this frame, so the list is not
        // built. The mouse wheel's turns and the commands' steps are
        // dropped. A wheel that closed while its key was held may open
        // again, because the key is now let go.
        scroll.clear();
        wheel.spare = 0.0;
        wheel.scripted.steps = 0;
        wheel.cancelled = false;
        return;
    }
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
    let groups = listed
        .as_ref()
        .map_or_else(Vec::new, |listed| listed.groups.clone());
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
        held,
        screen: ui.is_some_and(|ui| ui.stack.is_open()),
        talk: replies.is_some_and(|replies| replies.active),
        movie: showing.is_some_and(|showing| showing.0),
        body,
        mouse,
        stick: actions.as_ref().map_or(Vec2::ZERO, |actions| actions.aim),
        steps,
        arcs: &arcs,
        groups: &groups,
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
    let due = with_bar(snapshot, own.0, &swap.level, |means, rules| {
        let (content, pick) = listed.release(release)?;
        let target = crate::bar_view::cast_target(rules, content, foe, means.me.0);
        Some(Due::new(content, means, target, pick))
    });
    if let Some(due) = due.flatten() {
        wheel.queue(due);
    }
}

/// Plays out the first use in the queue: sends its pick and its actions,
/// and holds its buttons in `BarUse` beside the bar's. The next use starts
/// once it is over.
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
    let Some(due) = wheel.queue.front_mut() else {
        return;
    };
    let seen = presented
        .as_ref()
        .zip(own.as_ref())
        .zip(swap.as_ref())
        .and_then(|((presented, own), swap)| {
            let snapshot = presented.0.current();
            let (body, sheet) = crate::cast::body_sheet(snapshot, own.0, &swap.level)?;
            let picked = due.pick.and_then(|(shelf, _)| sheet.picks.item(shelf));
            let ready = body
                .fighter
                .as_ref()
                .is_some_and(|fighter| ready(due.content, fighter, snapshot.time_ms));
            Some(Seen {
                confirmed: due.pick.is_some_and(|(_, row)| picked == Some(row)),
                ready,
                now_ms: snapshot.time_ms,
            })
        });
    let Some(seen) = seen else {
        return;
    };
    let (sent, used) = due.step(seen, time.delta_secs());
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(sent);
    }
    bar_use.force = bar_use.force.or(used.force);
    bar_use.buttons |= used.buttons;
    bar_use.still |= used.still;
    if due.done() {
        wheel.queue.pop_front();
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
    let mine = library.and_then(|library| sim::traps::laid_kit(&snapshot.goods, library.items()));
    let shown = listed
        .list
        .iter()
        .zip(&listed.groups)
        .map(|(content, group)| {
            let kinds = match content {
                Content::Item(shelf) => listed.kinds[*shelf as usize].len(),
                _ => 0,
            };
            ui::vm::WheelEntry {
                group: *group,
                kinds: kinds.min(usize::from(u8::MAX)) as u8,
                ..crate::hud::bar_entry(ui, *content, means, &chosen, mine.as_ref())
            }
        })
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
                "arc": named(&view.arc), "arc_at": view.arc_at, "pointer": view.pointer,
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
        pointer: wheel.pointer.into(),
        bumpers: crate::pad::wheel_badges(actions.as_deref()),
    });
}

/// The wheel's rules, no window (`kjk sim probes`, group "power_wheel").
#[cfg(feature = "probes")]
pub mod probes;
