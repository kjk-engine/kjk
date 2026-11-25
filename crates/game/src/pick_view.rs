//! Which kind of item each key uses, from the player's side
//! (`sim::combat::picks`). An item kind's key is the key of the action bar
//! slot that holds the kind (`keys`). Hold that key and the mouse wheel (a
//! pad's D-pad left and right) moves through the kinds carried, sending
//! `Action::PickItem`, and a label over the slot names the one chosen. Let
//! go having moved, and nothing is used.
//!
//! A key is used on a press, which leaves no time to scroll, so with two
//! kinds or more the press is held back and a tap uses the kind as it is let
//! go. A grenade's tap readies the grenade or puts it away (`bar_view`), and
//! a grenade picked with the wheel is readied (the sim's `ready_grenades`).

use std::sync::Arc;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use sim::action::Action;
use sim::combat::bar::{Bar, Content};
use sim::combat::items::Shelf;
use sim::combat::{Fighter, Sheet};
use sim::inventory::PARTY;
use sim::notice::Notice;
use sim::{SimId, Snapshot};

use crate::loot::{Outgoing, words};
use crate::pad::Actions;
use crate::settings::{Action as Key, BAR};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// How many item kinds have a key, one for each `Shelf`.
const SHELVES: usize = Shelf::ALL.len();

/// Each item kind's key, by `Shelf`: the key of the bar slot that holds
/// the kind, if a slot does.
pub fn keys(bar: &Bar) -> [Option<Key>; SHELVES] {
    Shelf::ALL.map(|shelf| bar.slot_of(Content::Item(shelf)).map(|slot| BAR[slot]))
}

/// How long a tap's press is held down for the sim to see it (seconds: some
/// of its steps, whatever the frame rate).
pub(crate) const TAP: f32 = 0.04;
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
    held: [bool; SHELVES],
    pressed: [bool; SHELVES],
    /// How many kinds each key has.
    counts: [usize; SHELVES],
    steps: i32,
    seconds: f32,
}

/// What `Picking::frame` makes of it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Keyed {
    /// A key with kinds to choose among was pressed, or is moving.
    shelf: Option<Shelf>,
    steps: i32,
    /// Each key's state where it differs from the device's.
    set: [Option<bool>; SHELVES],
}

/// Where a run's commands turn the view to (`ui look at`, `ui look to`).
#[derive(Debug, Clone, PartialEq)]
enum LookAt {
    /// The nearest living creature with this tag.
    Tagged(String),
    /// A yaw and a pitch (degrees, pitch positive looking down).
    Angles { yaw: f32, pitch: f32 },
}

/// The turn of the view (degrees, x to the right and y down, as
/// `Look::turn` takes them) that brings the view of the body `seat` plays
/// in `snapshot` where `at` says. For a creature it brings the body's
/// crosshair's line onto the chest of the nearest living creature with the
/// tag (`sim::combat::mind::chest_of`). The line starts where the body's
/// last command put it (`mind::crosshair`), so it runs from the camera
/// beside the shoulder.
fn look_toward(
    snapshot: &Snapshot,
    seat: sim::Seat,
    level: &sim::Level,
    at: &LookAt,
) -> Option<Vec2> {
    use sim::combat::mind;
    let me = snapshot.body(seat)?.player?;
    let turn = |pitch: f32, yaw: f32| {
        Vec2::new(mind::turn(me.view[1], yaw), mind::turn(pitch, me.view[0]))
    };
    let tag = match at {
        LookAt::Angles { yaw, pitch } => return Some(turn(*pitch, *yaw)),
        LookAt::Tagged(tag) => tag,
    };
    let objects = sim::Instances::in_snapshot(level, snapshot);
    let apart = |them: &sim::motion::MotionState| {
        let d = sim::math::sub(them.origin, me.origin);
        sim::math::dot(d, d)
    };
    let (them, entity) = (snapshot.entities.iter())
        .filter(|e| crate::cast::is_tagged(e, &objects, tag))
        .filter(|e| e.fighter.is_some_and(|f| f.alive()))
        .filter_map(|e| Some((e.player?, e)))
        .min_by(|(a, _), (b, _)| apart(a).total_cmp(&apart(b)))?;
    let chest = mind::chest_of(&them, sim::bulk_of(objects.of_entity(entity)));
    let (pitch, yaw) = mind::aim(mind::crosshair(&me), chest);
    Some(turn(pitch, yaw))
}

/// The fighting keys a run's commands can hold (`ui key hold aim`), by
/// the word that names each: [Aim] raises a blaster, [Attack] fires it.
const SCRIPTED_FIGHT: [(&str, Key); 2] = [("aim", Key::Aim), ("attack", Key::Attack)];

/// The item key held, the presses due, what the player picked and what the
/// label says.
#[derive(Resource, Debug, Default)]
pub struct Picking {
    hold: Option<Hold>,
    tap: [f32; SHELVES],
    /// What the wheel has turned short of a notch.
    spare: f32,
    /// A pick the snapshots don't show yet: its row and when (seconds).
    sent: [Option<(u16, f32)>; SHELVES],
    /// The body the keys last played.
    body: Option<SimId>,
    label: Option<(LabelOn, Vec<u8>, f32)>,
    /// The keys a run's commands hold (`ui key`), the frame before's, the
    /// wheel they turned and the next-feat key they pressed: a headless run
    /// reads no device.
    held_by_script: [bool; SHELVES],
    was_by_script: [bool; SHELVES],
    wheel_by_script: f32,
    feat_by_script: bool,
    /// The bar slots a run's commands hold (`ui key hold slot N`), and the
    /// frame before's.
    slots_by_script: [bool; BAR.len()],
    slots_were: [bool; BAR.len()],
    /// The fighting keys a run's commands hold (`ui key hold aim`, `ui key
    /// hold attack`, in the order of `SCRIPTED_FIGHT`), and the frame
    /// before's.
    fight_by_script: [bool; SCRIPTED_FIGHT.len()],
    fight_were: [bool; SCRIPTED_FIGHT.len()],
    /// The degrees a run's commands turn the view by next frame (`ui look
    /// RIGHT DOWN`): x to the right, y down, as `Look::turn` takes them.
    look_by_script: Vec2,
    /// Where a run's commands turn the view to next: onto a creature
    /// (`ui look at TAG`) or to a yaw and a pitch (`ui look to YAW PITCH`).
    look_at: Option<LookAt>,
    /// A run's commands took the keys: its commands count though it has no
    /// window (`PlayerInput::read`).
    keys_scripted: bool,
    /// Each item kind's key on the bar of the body the keys last played
    /// (`keys`).
    keys: [Option<Key>; SHELVES],
}

impl Picking {
    /// An item key is held: it has the wheel.
    pub fn holding(&self) -> bool {
        self.hold.is_some()
    }

    /// The key held that has kinds to choose among. `pad` keeps this key
    /// held whatever the bumpers do, and its D-pad's left and right step
    /// through the kinds.
    pub(crate) fn choosing(&self) -> Option<Key> {
        let hold = self.hold.filter(|hold| hold.choice && !hold.cancelled)?;
        self.keys[hold.shelf as usize]
    }

    /// Medpacs on `key`, held with kinds to choose among (`pad`'s probes).
    #[cfg(feature = "probes")]
    pub(crate) fn choosing_medpacs(key: Key) -> Picking {
        let mut keys = [None; SHELVES];
        keys[Shelf::Medpac as usize] = Some(key);
        Picking {
            hold: Some(Hold {
                shelf: Shelf::Medpac,
                moved: false,
                deferred: true,
                choice: true,
                cancelled: false,
            }),
            keys,
            ..Picking::default()
        }
    }

    /// Nothing is going on: no key held, no press due, no pick on its way.
    fn idle(&self, actions: &Actions) -> bool {
        self.hold.is_none()
            && self.tap.iter().all(|tap| *tap <= 0.0)
            && self.sent.iter().all(Option::is_none)
            && BAR.iter().all(|key| !actions.held(*key))
    }

    /// The item kind whose key is held, while a screen or another body has
    /// not come between. While the player picks among two or more items of
    /// the kind, the key's press is held back from the bar, and the HUD
    /// uses this to show the kind's bar slot as pressed.
    pub fn held_shelf(&self) -> Option<Shelf> {
        let hold = self.hold.filter(|hold| !hold.cancelled)?;
        Some(hold.shelf)
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

    /// A command's request: `key hold grenade`, `key up grenade`, `key hold
    /// slot 2`, `key up slot 2`, `key hold aim`, `key up attack`, `key feat`,
    /// `wheel 1` (notches, down), `look 10 -5` (the view turned 10° to the
    /// right and 5° up), `look to 90 -5` (the view turned to a yaw of 90°
    /// and 5° up) or `look at g_sithtroop01` (the view turned so the
    /// crosshair's line meets the chest of the nearest living creature with
    /// that tag, `look_toward`).
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
            ["key", verb @ ("hold" | "up"), "slot", number] => match number.parse::<usize>() {
                Ok(number) if (1..=BAR.len()).contains(&number) => {
                    self.slots_by_script[number - 1] = *verb == "hold";
                }
                _ => return false,
            },
            ["key", verb @ ("hold" | "up"), name] => {
                let fight = SCRIPTED_FIGHT.iter().position(|(word, _)| word == name);
                match (on(name), fight) {
                    (Some(shelf), _) => self.held_by_script[shelf as usize] = *verb == "hold",
                    (None, Some(fight)) => self.fight_by_script[fight] = *verb == "hold",
                    (None, None) => return false,
                }
            }
            ["look", "at", tag] => self.look_at = Some(LookAt::Tagged(tag.to_string())),
            ["look", "to", yaw, pitch] => match (yaw.parse::<f32>(), pitch.parse::<f32>()) {
                (Ok(yaw), Ok(pitch)) if yaw.is_finite() && pitch.is_finite() => {
                    self.look_at = Some(LookAt::Angles { yaw, pitch });
                }
                _ => return false,
            },
            ["look", right, down] => match (right.parse::<f32>(), down.parse::<f32>()) {
                (Ok(right), Ok(down)) if right.is_finite() && down.is_finite() => {
                    self.look_by_script += Vec2::new(right, down);
                }
                _ => return false,
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

    /// What the run's commands hold, put in `actions`, with the view's turn
    /// they asked for; the wheel they turned.
    fn scripted(&mut self, actions: &mut Actions) -> f32 {
        let shelves = self.held_by_script.into_iter().zip(self.was_by_script);
        let slots = self.slots_by_script.into_iter().zip(self.slots_were);
        let kinds = self.keys.into_iter().zip(shelves);
        let bar = BAR.map(Some).into_iter().zip(slots);
        let fight = (SCRIPTED_FIGHT.map(|(_, key)| Some(key)).into_iter())
            .zip(self.fight_by_script.into_iter().zip(self.fight_were));
        for (key, (held, was)) in kinds.chain(bar).chain(fight) {
            if let Some(key) = key.filter(|_| held) {
                actions.set_held(key, true);
                if !was {
                    actions.set_pressed(key);
                }
            }
        }
        // A tap: held for this frame too, so that a single blade's style
        // cycles (`sim_host::player_command`).
        if std::mem::take(&mut self.feat_by_script) {
            actions.set_held(Key::NextFeat, true);
            actions.set_pressed(Key::NextFeat);
        }
        self.was_by_script = self.held_by_script;
        self.slots_were = self.slots_by_script;
        self.fight_were = self.fight_by_script;
        actions.turn += std::mem::take(&mut self.look_by_script);
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
        self.spare = 0.0;
    }

    /// One frame of the keys. `shut` says that the power wheel is open and
    /// has the keys. While it does, whatever is due is dropped (`cancel`)
    /// and no key is read. A key pressed meanwhile starts no hold, so
    /// letting it go after the wheel closes makes no tap.
    fn step(&mut self, shut: bool, frame: Frame) -> Keyed {
        if shut {
            self.cancel();
            return Keyed::default();
        }
        self.frame(frame)
    }

    fn frame(&mut self, frame: Frame) -> Keyed {
        let mut out = Keyed::default();
        for (tap, set) in self.tap.iter_mut().zip(&mut out.set) {
            if *tap > 0.0 {
                *set = Some(true);
                *tap -= frame.seconds;
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
                    self.hold = Some(Hold {
                        shelf,
                        moved: false,
                        deferred: choice,
                        choice,
                        cancelled: false,
                    });
                    if choice {
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
) -> [Vec<Kind>; SHELVES] {
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
            |row| library.item_row(row),
        )
    });
    let shown = sheet.items.iter().flatten().copied();
    let rows = carried.iter().copied().chain(
        shown
            .clone()
            .filter(|kind| carried.iter().all(|known| known.row != kind.row)),
    );
    let mut out: [Vec<Kind>; SHELVES] = Default::default();
    for kind in rows {
        let on = kind.kind();
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
    kinds: &[Vec<Kind>; SHELVES],
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
    let mut name = item_name(ui, kind)?;
    name.extend(format!("  x{}", kind.left).into_bytes());
    Some(name)
}

/// The name of `kind`'s item, else the name of the power it casts.
pub(crate) fn item_name(ui: &mut Ui, kind: &Kind) -> Option<Vec<u8>> {
    let name = kind.template.as_ref().and_then(|template| {
        let look = ui.assets.as_mut()?.item_ref(template)?;
        Some(look.name.clone())
    });
    name.or_else(|| table_name(ui, "spells", usize::from(kind.row)))
}

/// The talk table's line that row `row` of the 2DA table `table` names in
/// its `name` column (`spells`, `feat`, `skills`).
pub(crate) fn table_name(ui: &mut Ui, table: &str, row: usize) -> Option<Vec<u8>> {
    let strref = ui.assets.as_mut()?.cell(table, row, "name")?.parse().ok()?;
    Some(fmt_odyssey::code_page_bytes(&words(
        &Notice::Line(strref),
        Some(ui),
    )))
}

/// How far the mouse wheel turned this frame, in notches up. A trackpad's
/// scroll counts `NOTCH_PIXELS` pixels as a notch.
pub(crate) fn turned(wheel: &mut MessageReader<MouseWheel>) -> f32 {
    wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / NOTCH_PIXELS,
        })
        .sum()
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
    (time, power_wheel): (Res<Time<Real>>, Option<Res<crate::wheel_view::Wheel>>),
) {
    let turned = turned(&mut wheel);
    let busy = busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    // A run's `look at TAG`: the turn onto the creature, from the own body.
    if let Some(at) = picking.look_at.take() {
        let toward = (presented
            .as_deref()
            .zip(own.as_deref())
            .zip(swap.as_deref()))
        .and_then(|((presented, own), swap)| {
            look_toward(presented.0.current(), own.0, &swap.level, &at)
        });
        match toward {
            Some(turn) => picking.look_by_script += turn,
            None => warn!(target: "console", "ui look: nothing to turn to for {at:?}"),
        }
    }
    let body = (!busy)
        .then_some(())
        .and_then(|()| Some((presented?, own?, swap?)));
    let body = body.as_ref().and_then(|(presented, own, swap)| {
        let snapshot = presented.0.current();
        let (body, sheet) = crate::cast::body_sheet(snapshot, own.0, &swap.level)?;
        Some((
            snapshot,
            body.id,
            sheet,
            body.fighter?,
            swap.level.scripts(),
        ))
    });
    if let Some((_, _, sheet, ..)) = &body {
        picking.keys = keys(&sheet.bar);
    }
    let turned = turned + picking.scripted(&mut actions);
    if picking.idle(&actions) {
        return;
    }
    let Some((snapshot, id, sheet, fighter, library)) = body else {
        picking.cancel();
        return;
    };
    let now_ms = snapshot.time_ms;
    let kinds = kinds(snapshot, id, &sheet, &fighter, library);
    if picking.body.replace(id).is_some_and(|was| was != id) {
        picking.cancel();
    }
    let now = time.elapsed_secs();
    for shelf in Shelf::ALL {
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
        held: picking
            .keys
            .map(|key| key.is_some_and(|key| actions.held(key))),
        pressed: picking
            .keys
            .map(|key| key.is_some_and(|key| actions.pressed(key))),
        counts: std::array::from_fn(|at| kinds[at].len()),
        steps: i32::from(actions.pick_step) - notches as i32,
        seconds: time.delta_secs(),
    };
    // The power wheel open has the item keys too, so a key pressed then
    // starts nothing.
    let wheeling = power_wheel.is_some_and(|wheel| wheel.open);
    let keyed = picking.step(wheeling, frame);
    for (key, set) in picking.keys.into_iter().zip(keyed.set) {
        if let (Some(key), Some(on)) = (key, set) {
            actions.set_held(key, on);
        }
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
        ("the label fades", label_fades()),
        ("a cancelled hold does nothing more", cancelled_hold()),
        ("a bar slot is its item kind's key", bar_slot_keys()),
        (
            "a slot key while the wheel is open",
            slot_key_while_wheel_open(),
        ),
    ]
}

/// A key of medpacs with two kinds, pressed while the power wheel is open
/// and held for 3 frames, then held 3 frames more after the wheel closes,
/// then let go for 3 frames, is never held down for the sim, so no medpac
/// is used.
#[cfg(feature = "probes")]
fn slot_key_while_wheel_open() -> Result<String, String> {
    let at = Shelf::Medpac as usize;
    let script = [(true, true, true)]
        .into_iter()
        .chain([(true, true, false); 2])
        .chain([(false, true, false); 3])
        .chain([(false, false, false); 3]);
    let mut picking = Picking::default();
    let seen: Vec<bool> = script
        .map(|(shut, held, pressed)| {
            let mut frame = Frame {
                seconds: 0.01,
                ..Frame::default()
            };
            frame.held[at] = held;
            frame.pressed[at] = pressed;
            frame.counts[at] = 2;
            picking.step(shut, frame).set[at] == Some(true)
        })
        .collect();
    sim::probe::check(
        seen.iter().all(|held| !held),
        format!("the medpac key held down for the sim each frame {seen:?}"),
    )
}

/// Grenades put in bar slot 3 and medpacs in slot 1 make those slots'
/// keys the two kinds' keys, and the other kinds have none. A run's
/// `key hold grenade` then holds and presses slot 3's key, and
/// `key hold stim` holds nothing. `key hold slot 5` holds and presses slot
/// 5's key, whatever the slot holds, and `key hold slot 11` is refused.
/// `key hold aim` and `key hold attack` hold and press [Aim] and [Attack].
#[cfg(feature = "probes")]
fn bar_slot_keys() -> Result<String, String> {
    let mut bar = Bar::default();
    bar.put(2, Content::Item(Shelf::Grenade));
    bar.put(0, Content::Item(Shelf::Medpac));
    let mut picking = Picking {
        keys: keys(&bar),
        ..Picking::default()
    };
    let want = [Some(BAR[0]), None, None, Some(BAR[2])];
    let commands: [&[&str]; 4] = [
        &["key", "hold", "grenade"],
        &["key", "hold", "stim"],
        &["key", "hold", "slot", "5"],
        &["key", "hold", "slot", "11"],
    ];
    let scripted = commands.map(|words| picking.script(words));
    let mut actions = Actions::default();
    picking.scripted(&mut actions);
    let held = BAR.map(|key| actions.held(key));
    let pressed = BAR.map(|key| actions.pressed(key));
    let third_and_fifth = |seen: [bool; sim::combat::bar::SLOTS]| {
        seen == std::array::from_fn(|slot| [2, 4].contains(&slot))
    };
    let mut fighting = Picking::default();
    let fights: [&[&str]; 2] = [&["key", "hold", "aim"], &["key", "hold", "attack"]];
    let fought = fights.map(|words| fighting.script(words));
    let mut fight = Actions::default();
    fighting.scripted(&mut fight);
    let fight = [Key::Aim, Key::Attack].map(|key| (fight.held(key), fight.pressed(key)));
    sim::probe::check(
        picking.keys == want
            && scripted == [true, true, true, false]
            && third_and_fifth(held)
            && third_and_fifth(pressed)
            && fought == [true, true]
            && fight == [(true, true); 2],
        format!(
            "keys {:?}, held {held:?}, pressed {pressed:?}, aim and attack (held, pressed) {fight:?}",
            picking.keys
        ),
    )
}

/// Runs `script` (the key's state, the wheel's steps) on `shelf`'s key with
/// `count` kinds, then two frames with the key up: what the sim sees of the
/// key each frame, and the steps taken.
#[cfg(feature = "probes")]
fn run(shelf: Shelf, count: usize, script: &[(bool, i32)]) -> (Vec<bool>, Vec<i32>) {
    let mut picking = Picking::default();
    let (mut seen, mut moved) = (Vec::new(), Vec::new());
    let at = shelf as usize;
    let mut was = false;
    for (held, steps) in script.iter().chain(&[(false, 0); 2]) {
        let mut frame = Frame {
            steps: *steps,
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

/// Runs `script` with `count` kinds on every kind's key, grenades
/// included: the kinds whose run `right` refuses, with what the sim saw
/// of the key and the steps taken.
#[cfg(feature = "probes")]
fn every_kind(
    count: usize,
    script: &[(bool, i32)],
    right: impl Fn(&[bool], &[i32]) -> bool,
) -> Result<String, String> {
    let wrong: Vec<String> = Shelf::ALL
        .into_iter()
        .filter_map(|shelf| {
            let (seen, moved) = run(shelf, count, script);
            (!right(&seen, &moved)).then(|| format!("{shelf:?}: {seen:?} {moved:?}"))
        })
        .collect();
    sim::probe::check(
        wrong.is_empty(),
        format!("{} kinds, wrong {wrong:?}", Shelf::ALL.len()),
    )
}

/// A key with one kind is pressed for the sim as it goes down. For the
/// grenades the press readies the grenade (`bar_view`).
#[cfg(feature = "probes")]
fn one_kind_on_press() -> Result<String, String> {
    every_kind(1, &TAPPED, |seen, _| seen[0] && presses(seen) == 1)
}

/// A key with two kinds is pressed for the sim as it is let go, since
/// holding it lets the player scroll to another kind.
#[cfg(feature = "probes")]
fn two_kinds_on_let_go() -> Result<String, String> {
    every_kind(2, &TAPPED, |seen, _| {
        !seen[0] && !seen[1] && seen[2] && presses(seen) == 1
    })
}

/// A key held while the mouse wheel scrolls is never pressed for the sim.
#[cfg(feature = "probes")]
fn scrolling_uses_nothing() -> Result<String, String> {
    every_kind(3, &SCROLLED, |seen, moved| {
        presses(seen) == 0 && moved == [0, 1, 0, 0, 0]
    })
}

#[cfg(feature = "probes")]
fn one_kind_no_scroll() -> Result<String, String> {
    let (seen, moved) = run(Shelf::Stim, 1, &SCROLLED);
    sim::probe::check(
        presses(&seen) == 1 && moved.iter().all(|step| *step == 0),
        format!("{seen:?} {moved:?}"),
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
