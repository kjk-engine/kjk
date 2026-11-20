//! The action bar from the player's side (`sim::combat::bar`). Each of the
//! ten slot keys (`settings::BAR`) uses what its slot holds:
//!
//! - A Force power that the force button can cast is cast with that button
//!   while the key is held (`Command::force`). Any other power is cast on
//!   the press (`Action::Cast`), at the target for a power used on an enemy
//!   and at the character itself for any other power.
//! - An item kind is used with its button while the key is held. The item
//!   picker (`pick_view`) treats the key as that kind's key, so a tap uses
//!   the item and holding the key lets the player step through the kinds.
//! - Grenades are not thrown by their key. The key's press readies the
//!   grenade the slot uses, or puts it away when one is readied
//!   (`Action::Grenade`). While a grenade is readied, the attack button
//!   throws it and the weapon's other attack puts it away (the sim's
//!   `ready_grenades`).
//! - Mines lay a mine, and stealth mode turns stealth on or off, as the key
//!   goes down. Laying a mine stops a run: the movement keys move the
//!   character no more until the player lets go of them (`BarUse::still`),
//!   so the character stands still while it sets the mine.
//! - A feat attack is chosen for the weapon in hand on the press
//!   (`Action::Feat`).
//!
//! A slot's Force power or mine pressed while the character is busy, or
//! while uses from the power wheel wait, joins the wheel's queue of uses
//! (`wheel_view::Due`) and is carried out after them. The slot then does
//! nothing more until its key is let go.
//!
//! Only one slot is used at a time. The first key held wins until it is
//! let go. Nothing is used while a screen, a conversation or a movie is
//! up, or while the power wheel is open. A seat that flies the
//! remote droid has no bar: slot 1's key fires the droid's stim dart.

use bevy::prelude::*;
use sim::action::{Action, GrenadeOrder};
use sim::buttons;
use sim::combat::bar::{self, Content, Rules, SLOTS};
use sim::combat::items::{Readied, Shelf};
use sim::combat::picks::Picks;
use sim::combat::{Fighter, Sheet, force};
use sim::{Seat, SimId, Snapshot};

use crate::loot::Outgoing;
use crate::pad::Actions;
use crate::settings::{Action as Key, BAR};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};

/// What the bar asks of this frame's commands.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct BarUse {
    /// The power the force button uses (`Command::force`, a `spells.2da`
    /// row), while a slot with that power is held.
    pub force: Option<u16>,
    /// The button bits the slot in use holds down (`sim::input::buttons`).
    pub buttons: u16,
    /// The actions the slot in use sends this frame.
    pub actions: Vec<Action>,
    /// The slot in use, from 0, whether or not the character can use what
    /// it holds now. The HUD shows it pressed.
    pub held: Option<usize>,
    /// A mine is being laid. The command leaves out the movement keys until
    /// the player lets go of them, so a run stops and the lay is not broken
    /// off by the character moving away.
    pub still: bool,
}

/// What the character whose bar it is can use now (`usable`).
pub struct Means<'a> {
    pub sheet: Sheet,
    pub fighter: &'a Fighter,
    pub me: SimId,
    /// What each power costs the character where it stands now, in whole
    /// Force points (`ModuleData::power_cost`). None where the module lists
    /// no cost.
    pub cost: &'a dyn Fn(u16) -> Option<i32>,
    /// Whether the items the character wears bar a power, by its
    /// `spells.2da` row (`ModuleData::power_barred`). Such a power is
    /// greyed and its slot does nothing, as the original radial menu greys
    /// it and the cast is refused.
    pub barred: &'a dyn Fn(u16) -> bool,
    /// The powers the force button can cast (`force::choices`).
    pub choices: Vec<u16>,
    /// What the character can put on the bar now (`bar::assignable`).
    pub assignable: Vec<Content>,
    pub now_ms: u64,
}

/// Whether the character has the Force points to start the power that
/// `content` names. The HUD dims a slot whose power the character has too
/// few Force points to start. Pull uses as many Force points as Push
/// (`force::data_row`). Any other content, and a power whose cost the
/// module does not list, counts as affordable.
///
/// This is never a reason to hold back a slot's press or hold. The sim
/// refuses a cast the character has too few Force points for. A channelled
/// power held on takes only each step's smaller cost
/// (`force::channel_cost`), and a running toggle such as Force Speed turns
/// off with no cost, so the client's check of the whole cost would stop
/// both too early.
pub fn payable(content: Content, means: &Means) -> bool {
    let Content::Power(row) = content else {
        return true;
    };
    (means.cost)(force::data_row(row)).is_none_or(|cost| {
        sim::combat::effects::can_pay(
            &means.fighter.effects.lasting,
            (means.fighter.fp, means.fighter.hp),
            cost * 100,
            means.now_ms,
        )
    })
}

/// Runs `run` with what the body `seat` plays can use now, and with the
/// bar's rules for its level. The rules know whether the party carries a
/// mine kit. None when the seat plays no body with a fight.
pub(crate) fn with_bar<R>(
    snapshot: &Snapshot,
    seat: Seat,
    level: &sim::Level,
    run: impl FnOnce(&Means, &Rules) -> R,
) -> Option<R> {
    with_bar_of(snapshot, snapshot.body(seat)?, level, run)
}

/// Runs `run` as `with_bar` does, for the party member `body`, whichever
/// seat plays it. The bar setup screen shows the member the menu shows.
pub(crate) fn with_bar_of<R>(
    snapshot: &Snapshot,
    body: &sim::EntityState,
    level: &sim::Level,
    run: impl FnOnce(&Means, &Rules) -> R,
) -> Option<R> {
    let sheet = crate::cast::worn_sheet(snapshot, body, level);
    let fighter = body.fighter.as_ref()?;
    let library = level.scripts().map(|library| &**library);
    let mines =
        library.is_some_and(|library| sim::traps::party_has_kit(&snapshot.goods, library.items()));
    let cost = |row| level.module()?.power_cost(row, &sheet, true, body.position);
    let barred = |row| {
        level
            .module()
            .is_some_and(|module| module.power_barred(row, sheet.gear.force_items))
    };
    Some(Rules::with(library, mines, |rules| {
        let means = Means {
            sheet,
            fighter,
            me: body.id,
            cost: &cost,
            barred: &barred,
            choices: force::choices(&sheet.powers, library),
            assignable: bar::assignable(&sheet, rules),
            now_ms: snapshot.time_ms,
        };
        run(&means, rules)
    }))
}

/// Whether the character can use `content` now. A power must be one it can
/// cast, and not one the items it wears bar (`Means::barred`). Its cost is
/// left to the sim (`payable`). An item kind must have
/// an item left that the sim's key would use. A feat attack must be known
/// and fit the weapon in hand. Mines need a mine kit and a rank in
/// Demolitions. Stealth mode needs a stealth field generator and a rank in
/// Stealth (`bar::assignable`).
pub fn usable(content: Content, means: &Means) -> bool {
    let (sheet, fighter) = (&means.sheet, means.fighter);
    match content {
        Content::Power(row) => {
            means.assignable.contains(&content) && !(means.barred)(force::data_row(row))
        }
        Content::Item(shelf) => fighter.pack.keyed(sheet, shelf, means.now_ms).is_some(),
        // A double-bladed weapon with one blade lit fights as a single
        // blade, which has no feat attack to choose.
        Content::Feat(line) => {
            !fighter.saber.second_off
                && line.known(sheet).is_some()
                && line.fits(sheet, fighter.hand)
        }
        Content::Mine | Content::Stealth => means.assignable.contains(&content),
    }
}

/// Whether the slot holding `content` shows as on: the feat attack the
/// weapon in hand uses now (a gun's first known line too), stealth mode
/// while the character is in stealth, or the grenades while the character
/// has a grenade readied.
pub fn lit(content: Content, sheet: &Sheet, fighter: &Fighter) -> bool {
    match content {
        Content::Feat(line) => Picks::in_use(sheet, fighter.hand) == Some(line),
        Content::Stealth => fighter.covert.stealth,
        Content::Item(Shelf::Grenade) => fighter.pack.readied == Readied::On,
        Content::Power(_) | Content::Item(_) | Content::Mine => false,
    }
}

/// Where a cast from the bar goes: a power used on an enemy at `foe` (none
/// without a target), any other power at the character itself (`me`).
pub fn cast_target(rules: &Rules, content: Content, foe: Option<u32>, me: u32) -> Option<u32> {
    match content {
        Content::Power(row) if rules.hostile(row) => foe,
        _ => Some(me),
    }
}

/// The body of the target the player has, by its sim id, for a power used
/// on an enemy (`cast_target`).
pub(crate) fn foe(
    targeted: Option<&crate::target_view::Targeted>,
    snapshot: &Snapshot,
) -> Option<u32> {
    targeted
        .and_then(|targeted| targeted.object)
        .and_then(|number| sim::target::entity(snapshot, number))
        .map(|entity| entity.id.0)
}

/// What a slot holding `content` does this frame, its key `pressed` and
/// `held`. `choices` are the powers the force button can cast, `hand` the
/// hand whose weapon a feat attack is chosen for and `target` where a
/// power cast on the press goes (`cast_target`). A slot the character
/// cannot use (`usable`) does nothing.
pub fn slot_use(
    content: Content,
    choices: &[u16],
    usable: bool,
    pressed: bool,
    held: bool,
    hand: u8,
    target: Option<u32>,
) -> BarUse {
    let mut out = BarUse::default();
    if !usable || !(pressed || held) {
        return out;
    }
    match content {
        Content::Power(row) if choices.contains(&row) => {
            out.force = Some(row);
            out.buttons = buttons::FORCE_POWER;
        }
        Content::Power(row) => {
            if let Some(target) = target.filter(|_| pressed) {
                out.actions.push(Action::Cast { row, target });
            }
        }
        // Grenades are readied on the press, or put away when one is
        // readied. The attack button then throws the grenade.
        Content::Item(Shelf::Grenade) => {
            if pressed {
                out.actions.push(Action::Grenade(GrenadeOrder::Toggle));
            }
        }
        Content::Item(shelf) => out.buttons = shelf.button(),
        // The sim lays a mine and turns stealth on or off as its button
        // goes down (`world::covert`), so the button is held with the key.
        Content::Mine => {
            out.buttons = buttons::LAY_MINE;
            out.still = true;
        }
        Content::Stealth => out.buttons = buttons::STEALTH,
        Content::Feat(line) => {
            if pressed {
                out.actions.push(Action::Feat { hand, line });
            }
        }
    }
    out
}

/// The slot in use this frame: `was` while its key stays held, else the
/// first slot whose key is held.
pub fn in_use(was: Option<usize>, held: [bool; SLOTS]) -> Option<usize> {
    was.filter(|slot| held[*slot])
        .or_else(|| held.iter().position(|on| *on))
}

/// This frame's slot in use (`in_use`) from each slot key's state, and
/// whether it counts as pressed: its key went down, or it took over from
/// the slot before. `was` is the slot in use the frame before.
pub fn step(
    was: &mut Option<usize>,
    held: [bool; SLOTS],
    pressed: [bool; SLOTS],
) -> Option<(usize, bool)> {
    let slot = in_use(*was, held);
    let taken = slot.map(|slot| (slot, pressed[slot] || *was != Some(slot)));
    *was = slot;
    taken
}

/// Each slot key held and pressed this frame, as the bar takes them. While
/// the bar is `closed` (a screen, a conversation or a movie is up, or the
/// power wheel is open), no slot key counts. A slot key held meanwhile then
/// does nothing until it is let go and pressed again (`pad::gate_keys`,
/// `kept`), so the key held as the wheel closes uses no slot.
fn held_back(closed: bool, kept: &mut u64, actions: &Actions) -> ([bool; SLOTS], [bool; SLOTS]) {
    let bits = |on: fn(&Actions, Key) -> bool| {
        BAR.iter()
            .filter(|key| on(actions, **key))
            .fold(0, |bits, key| bits | crate::pad::bit(*key))
    };
    let (held, pressed) = crate::pad::gate_keys(
        closed.then_some(0),
        kept,
        bits(Actions::held),
        bits(Actions::pressed),
    );
    let slots = |bits: u64| BAR.map(|key| bits & crate::pad::bit(key) != 0);
    (slots(held), slots(pressed))
}

/// The action bar is read into `BarUse`, which the commands come after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Used;

pub struct BarViewPlugin;

impl Plugin for BarViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BarUse>().add_systems(
            PreUpdate,
            use_bar
                .in_set(Used)
                .after(crate::pick_view::Picked)
                .after(crate::pad::Sampled),
        );
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_bar(
    actions: Option<Res<Actions>>,
    (replies, showing, ui): (
        Option<Res<crate::dialogue_view::ReplyInput>>,
        Option<Res<crate::movie_view::Showing>>,
        Option<Res<crate::ui_view::Ui>>,
    ),
    (presented, own, swap): (
        Option<Res<PresentedSnapshot>>,
        Option<Res<OwnSeat>>,
        Option<Res<SceneSwap>>,
    ),
    (targeted, mut wheel): (
        Option<Res<crate::target_view::Targeted>>,
        Option<ResMut<crate::wheel_view::Wheel>>,
    ),
    mut outgoing: Option<ResMut<Outgoing>>,
    mut bar_use: ResMut<BarUse>,
    (mut was, mut kept, mut watched, mut queued): (
        Local<Option<usize>>,
        Local<u64>,
        Local<bool>,
        Local<Option<usize>>,
    ),
) {
    // A mine's lay keeps the character still until the movement keys are
    // let go.
    let moving = actions
        .as_ref()
        .is_some_and(|actions| crate::pad::step(actions, false, || false) != (0, 0));
    let still = bar_use.still && moving;
    *bar_use = BarUse {
        still,
        ..BarUse::default()
    };
    // A movie that starts puts a readied grenade away. The sim knows of
    // conversations but not of movies, which only the client shows.
    let movie = showing.as_ref().is_some_and(|showing| showing.0);
    let started = movie && !*watched;
    *watched = movie;
    if started {
        let readied = presented
            .as_ref()
            .zip(own.as_ref())
            .is_some_and(|(presented, own)| {
                let body = presented.0.current().body(own.0);
                body.and_then(|body| body.fighter.as_ref())
                    .is_some_and(|fighter| fighter.pack.readied == Readied::On)
            });
        if readied && let Some(outgoing) = outgoing.as_mut() {
            outgoing.0.push(Action::Grenade(GrenadeOrder::PutAway));
        }
    }
    let Some(actions) = actions else {
        *was = None;
        return;
    };
    let busy = crate::pick_view::busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    let closed = busy || wheel.as_ref().is_some_and(|wheel| wheel.open);
    let (held, pressed) = held_back(closed, &mut kept, &actions);
    let taken = step(&mut was, held, pressed);
    // A slot whose press joined the queue does nothing more while its key
    // stays held.
    *queued = queued.filter(|slot| taken.is_some_and(|(at, pressed)| at == *slot && !pressed));
    if queued.is_some() {
        bar_use.held = taken.map(|(slot, _)| slot);
        return;
    }
    let (Some((slot, pressed)), Some(presented), Some(own), Some(swap)) =
        (taken, presented, own, swap)
    else {
        return;
    };
    bar_use.held = Some(slot);
    let snapshot = presented.0.current();
    if snapshot.body(own.0).is_none() && snapshot.remote(own.0).is_some() {
        // The droid's stim dart is its kit's third button, read as the
        // medpac button (`sim::remote`).
        if slot == 0 {
            bar_use.buttons = buttons::MEDPAC;
        }
        return;
    }
    let foe = foe(targeted.as_deref(), snapshot);
    let waiting = wheel.as_ref().is_some_and(|wheel| wheel.queued());
    let used = with_bar(snapshot, own.0, &swap.level, |means, rules| {
        let content = means.sheet.bar.slots()[slot]?;
        let target = cast_target(rules, content, foe, means.me.0);
        let usable = usable(content, means);
        let busy = !crate::wheel_view::ready(content, means.fighter, means.now_ms);
        if pressed && usable && queues(content) && (waiting || busy) {
            return Some(Err(crate::wheel_view::Due::new(
                content, means, target, None,
            )));
        }
        let hand = means.fighter.hand;
        Some(Ok(slot_use(
            content,
            &means.choices,
            usable,
            pressed,
            true,
            hand,
            target,
        )))
    });
    let used = match used.flatten() {
        Some(Ok(used)) => used,
        Some(Err(due)) => {
            if let Some(wheel) = wheel.as_mut() {
                wheel.queue(due);
                *queued = Some(slot);
            }
            return;
        }
        None => return,
    };
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(used.actions.iter().cloned());
    }
    *bar_use = BarUse {
        held: Some(slot),
        still: still || used.still,
        ..used
    };
}

/// Whether a slot's press of `content` joins the power wheel's queue of
/// uses while the character is busy or uses wait there: a Force power or a
/// mine. Items stay with the item picker (`pick_view`), which reads their
/// keys itself.
fn queues(content: Content) -> bool {
    matches!(content, Content::Power(_) | Content::Mine)
}

/// The bar's rules, no window (`kjk sim probes`, group "action_bar").
#[cfg(feature = "probes")]
pub mod probes {
    use super::*;
    use crate::pad::Taken;
    use crate::settings::KeyMap;
    use bevy::input::keyboard::KeyCode;
    use sim::combat::bar::FeatLine;
    use sim::combat::items::Shelf;
    use sim::combat::sheet::feats;
    use sim::combat::spells::{
        FORCE_POWER_FORCE_PUSH as PUSH, FORCE_POWER_HEAL as HEAL,
        FORCE_POWER_LIGHTNING as LIGHTNING, FORCE_POWER_SPEED_BURST as SPEED,
    };
    use sim::probe::{Checks, check};

    pub fn probes() -> Checks {
        vec![
            ("slot use", slot_uses()),
            ("unusable slots do nothing", unusable()),
            ("first held wins", first_held_wins()),
            ("lit slots", lit_slots()),
            ("usable slots", usable_slots()),
            ("powers the armour bars", barred_powers()),
            ("reply keys use no slot", reply_keys()),
            ("conversation keys", talk_keys()),
            ("wheel keys use no slot", wheel_keys()),
            ("held powers ignore the start cost", held_powers()),
            ("Force Pull reads Force Push", pull_reads_push()),
            (
                "power descriptions quote the rulebook",
                rulebook_in_descriptions(),
            ),
            ("readied grenades", readied_grenades()),
        ]
    }

    /// A step of a readied grenade's case: the buttons the player holds
    /// down, the actions the seat sends, and whether the player restrict
    /// mode is on.
    type Step = (u16, Vec<Action>, bool);

    /// A readied grenade's case: its name, its steps after the start, and
    /// at the end whether a grenade is readied, whether the weapon is drawn
    /// (none: not checked), and whether a grenade was thrown.
    type ReadiedCase = (String, Vec<Step>, Readied, Option<bool>, bool);

    fn press(buttons: u16) -> Step {
        (buttons, Vec::new(), false)
    }

    fn send(action: Action) -> Step {
        (0, vec![action], false)
    }

    /// The dev soldier (three fragmentation grenades and a thermal
    /// detonator as its first two kinds, then medpacs and an energy
    /// shield) on the proving ground, stepped through `steps`, 1.6 s a
    /// step. Its pack and its weapon's sheath at the end.
    fn readied_run(
        steps: &[Step],
    ) -> Result<(sim::combat::items::Pack, sim::combat::Sheath), String> {
        let config = sim::WorldConfig {
            sheet: Sheet::dev_soldier(),
            ..sim::WorldConfig::one_seat()
        };
        let mut world = sim::SimWorld::new(config, sim::determinism::proving_ground())
            .map_err(|error| error.to_string())?;
        let mut last = None;
        for (tick, (held, actions, restrict)) in steps.iter().enumerate() {
            let mut state = world.script_state().clone();
            state.party.restrict = *restrict;
            world.set_script_state(state);
            let mut input = sim::TickInput::default();
            input.insert(
                Seat(0),
                sim::Command {
                    buttons: *held,
                    ..sim::Command::default()
                },
            );
            for action in actions {
                input.act(Seat(0), action.clone());
            }
            let snapshot = sim::step(
                &mut world,
                tick as u64 + 1,
                &input,
                1600,
                sim::StepReason::Authority,
            );
            last = snapshot.body(Seat(0)).and_then(|body| body.fighter);
        }
        let fighter = last.ok_or("no body with a fight")?;
        Ok((fighter.pack, fighter.sheath))
    }

    /// The grenade readied from the bar's slot or the wheel's entry, or by
    /// picking a grenade, is thrown with the attack button and put away by
    /// the other attack button and by every other use. Each case starts
    /// with the weapon drawn (the holster key from the weapon put away),
    /// except where the player holstered it before readying. After a throw
    /// or a put-away the weapon is drawn again, but not after the holster
    /// key, and not when it was holstered before the grenade was readied.
    fn readied_grenades() -> Result<String, String> {
        use sim::combat::bar::FeatLine;
        let draw = || send(Action::Holster);
        let ready = || send(Action::Grenade(GrenadeOrder::Toggle));
        let settle = || press(0);
        let readied = |name: &str, last: Step| {
            (
                name.to_string(),
                vec![draw(), last],
                Readied::On,
                Some(false),
                false,
            )
        };
        let mut cases: Vec<ReadiedCase> = vec![
            readied("readied from a slot or the wheel", ready()),
            readied(
                "readied by picking a grenade",
                send(Action::PickItem { row: 89 }),
            ),
            (
                "thrown with the attack button".into(),
                vec![draw(), ready(), press(buttons::ATTACK), settle()],
                Readied::Off,
                Some(true),
                true,
            ),
            (
                "holstered before, thrown, still holstered".into(),
                vec![draw(), draw(), ready(), press(buttons::ATTACK), settle()],
                Readied::Off,
                Some(false),
                true,
            ),
        ];
        // Each way of putting the readied grenade away without a throw,
        // and whether the weapon is drawn again after it.
        let away: Vec<(&str, Step, Option<bool>)> = vec![
            (
                "the other attack button",
                press(buttons::ALT_ATTACK),
                Some(true),
            ),
            ("grenades chosen again", ready(), Some(true)),
            (
                "a movie starting (the put-away order)",
                send(Action::Grenade(GrenadeOrder::PutAway)),
                Some(true),
            ),
            ("the holster key", draw(), Some(false)),
            ("switching weapon sets", send(Action::SwitchWeapons), None),
            ("the player restrict mode", (0, Vec::new(), true), None),
            (
                "a feat attack",
                send(Action::Feat {
                    hand: HAND,
                    line: FeatLine::PowerBlast,
                }),
                None,
            ),
            (
                "a Force power cast",
                send(Action::Cast {
                    row: HEAL,
                    target: ME,
                }),
                None,
            ),
            ("the Force power button", press(buttons::FORCE_POWER), None),
            ("a medpac", press(buttons::MEDPAC), None),
            ("an energy shield", press(buttons::SHIELD), None),
            ("a stim", press(buttons::STIM), None),
            ("stealth mode", press(buttons::STEALTH), None),
            ("mines", press(buttons::LAY_MINE), None),
        ];
        cases.extend(away.into_iter().map(|(name, step, drawn)| {
            (
                format!("put away by {name}"),
                vec![draw(), ready(), step, settle()],
                Readied::Off,
                drawn,
                false,
            )
        }));
        let mut wrong = Vec::new();
        for (name, steps, want, drawn, thrown) in &cases {
            let (pack, sheath) = readied_run(steps)?;
            let was_drawn = sheath == sim::combat::Sheath::Drawn;
            // The soldier's grenades are its first two kinds.
            let threw = pack.used[..2].iter().any(|used| *used > 0);
            if pack.readied != *want
                || drawn.is_some_and(|drawn| drawn != was_drawn)
                || threw != *thrown
            {
                wrong.push(format!(
                    "{name}: {:?}, {sheath:?}, used {:?}",
                    pack.readied, pack.used
                ));
            }
        }
        check(
            wrong.is_empty(),
            format!("{} cases, wrong {wrong:?}", cases.len()),
        )
    }

    /// A frame of slot 3's key (Bar3, the number 3 by default): its name,
    /// whether something else has the keys (a conversation, or the power
    /// wheel), whether the key is down and whether it went down this frame,
    /// and whether the slot is used.
    type KeyFrame = (&'static str, bool, bool, bool, bool);

    const REPLY_FRAMES: [KeyFrame; 6] = [
        ("the reply picked", true, true, true, false),
        ("the key held in the conversation", true, true, false, false),
        (
            "the conversation ended, the key held",
            false,
            true,
            false,
            false,
        ),
        ("the key still held", false, true, false, false),
        ("the key let go", false, false, false, false),
        ("the key pressed again", false, true, true, true),
    ];

    const WHEEL_FRAMES: [KeyFrame; 5] = [
        (
            "the key pressed with the wheel open",
            true,
            true,
            true,
            false,
        ),
        ("the key held with the wheel open", true, true, false, false),
        ("the wheel closed, the key held", false, true, false, false),
        ("the key let go", false, false, false, false),
        ("the key pressed again", false, true, true, true),
    ];

    /// Slot 3's keys this frame after a gate: whether something else has
    /// the keys, what the gate keeps held back, and the key down and gone
    /// down.
    type Gate = dyn Fn(bool, &mut u64, bool, bool) -> ([bool; SLOTS], [bool; SLOTS]);

    /// `frames` through `gate` and then the bar (`step`, `slot_use`), with
    /// slot 3 holding medpacs and then a friendly power the force button
    /// cannot cast: the frames whose use is not as listed.
    fn run_frames(frames: &[KeyFrame], gate: &Gate) -> Vec<String> {
        let mut wrong = Vec::new();
        for content in [Content::Item(Shelf::Medpac), Content::Power(HEAL)] {
            let (mut kept, mut was) = (0, None);
            for (name, closed, down, went, uses) in frames {
                let (held, pressed) = gate(*closed, &mut kept, *down, *went);
                let used = step(&mut was, held, pressed)
                    .filter(|(slot, _)| *slot == 2)
                    .map_or(BarUse::default(), |(_, pressed)| {
                        used(content, false, true, (pressed, true), Some(FOE))
                    });
                if (used != BarUse::default()) != *uses {
                    wrong.push(format!("{content:?}, {name}: {used:?}"));
                }
            }
        }
        wrong
    }

    /// The keys pass the screens' gate (`pad::gate_keys`) with the default
    /// keys. The number that picks a reply uses no slot, nor does it while
    /// still held as play resumes. Let go and pressed again, it uses the
    /// slot.
    fn reply_keys() -> Result<String, String> {
        let gate = |talk: bool, kept: &mut u64, down: bool, went: bool| {
            let key = crate::pad::bit(BAR[2]);
            let bits = |on: bool| if on { key } else { 0 };
            let taken = if talk { Taken::Talk } else { Taken::Play };
            let open = crate::pad::open_keys(taken, KeyMap::defaults());
            let (held, pressed) = crate::pad::gate_keys(open, kept, bits(down), bits(went));
            let slots = |bits: u64| BAR.map(|key| bits & crate::pad::bit(key) != 0);
            (slots(held), slots(pressed))
        };
        let wrong = run_frames(&REPLY_FRAMES, &gate);
        check(
            wrong.is_empty(),
            format!(
                "{} frames for medpacs and Heal, wrong {wrong:?}",
                REPLY_FRAMES.len()
            ),
        )
    }

    /// A slot key held while the power wheel is open (`held_back`) uses no
    /// slot as the wheel closes. Let go and pressed again, it uses the slot.
    fn wheel_keys() -> Result<String, String> {
        let gate = |wheel: bool, kept: &mut u64, down: bool, went: bool| {
            let mut actions = Actions::default();
            actions.set_held(BAR[2], down);
            if went {
                actions.set_pressed(BAR[2]);
            }
            held_back(wheel, kept, &actions)
        };
        let wrong = run_frames(&WHEEL_FRAMES, &gate);
        check(
            wrong.is_empty(),
            format!(
                "{} frames for medpacs and Heal, wrong {wrong:?}",
                WHEEL_FRAMES.len()
            ),
        )
    }

    /// An action during a conversation: its name, what has the keys, the
    /// key it is bound to instead of its default, and whether it comes
    /// through.
    type TalkCase = (&'static str, Taken, Key, Option<KeyCode>, bool);

    const TALK_CASES: [TalkCase; 8] = [
        (
            "accept call in a conversation",
            Taken::Talk,
            Key::AcceptCall,
            None,
            true,
        ),
        (
            "end call in a conversation",
            Taken::Talk,
            Key::EndCall,
            None,
            true,
        ),
        (
            "ready in a conversation",
            Taken::Talk,
            Key::Ready,
            None,
            true,
        ),
        (
            "break in a conversation",
            Taken::Talk,
            Key::Break,
            None,
            true,
        ),
        ("ready on a screen", Taken::Screens, Key::Ready, None, false),
        (
            "ready rebound to the reply number 1",
            Taken::Talk,
            Key::Ready,
            Some(KeyCode::Digit1),
            false,
        ),
        ("slot 3 in a conversation", Taken::Talk, BAR[2], None, false),
        ("slot 7 in a conversation", Taken::Talk, BAR[6], None, false),
    ];

    /// Which actions a conversation leaves to the keys (`pad::open_keys`):
    /// answering a join request, a break and a ready check do come through,
    /// but not on a screen, and not on a key the conversation reads.
    fn talk_keys() -> Result<String, String> {
        let wrong: Vec<&str> = TALK_CASES
            .iter()
            .filter(|(_, taken, action, rebound, passes)| {
                let keys = match rebound {
                    Some(key) => KeyMap::defaults().clone().with_key(*action, *key),
                    None => KeyMap::defaults().clone(),
                };
                let bit = crate::pad::bit(*action);
                let mut kept = 0;
                let (held, pressed) = crate::pad::gate_keys(
                    crate::pad::open_keys(*taken, &keys),
                    &mut kept,
                    bit,
                    bit,
                );
                (held == bit && pressed == bit) != *passes
            })
            .map(|(name, ..)| *name)
            .collect();
        check(
            wrong.is_empty(),
            format!("{} cases, wrong {wrong:?}", TALK_CASES.len()),
        )
    }

    /// The Force points to start Force Lightning (held on) and Force Speed
    /// (pressed to turn it off) are short, yet both slots still hold the
    /// force button with their power, because the sim decides what they
    /// cost. Only the HUD's `payable` says they cannot be paid for.
    fn held_powers() -> Result<String, String> {
        let (mut sheet, fighter) = soldier();
        let cases = [
            ("lightning held on", LIGHTNING, false),
            ("speed pressed", SPEED, true),
        ];
        for (_, row, _) in cases {
            sheet.powers.insert(row);
        }
        let wrong: Vec<String> = with_means(sheet, &fighter, &|_| Some(50), |means| {
            cases
                .iter()
                .filter_map(|(name, row, pressed)| {
                    let content = Content::Power(*row);
                    let usable = usable(content, means);
                    let used =
                        slot_use(content, &means.choices, usable, *pressed, true, HAND, None);
                    let want = uses(Some(*row), buttons::FORCE_POWER, vec![]);
                    (used != want || payable(content, means)).then(|| format!("{name}: {used:?}"))
                })
                .collect()
        });
        check(
            wrong.is_empty(),
            format!("Force points 0 of 50, wrong {wrong:?}"),
        )
    }

    /// Runs `run` with what the character of `sheet` and `fighter` can use,
    /// with JA's powers (no game data) and the Force point costs `cost`.
    fn with_means<R>(
        sheet: Sheet,
        fighter: &Fighter,
        cost: &dyn Fn(u16) -> Option<i32>,
        run: impl FnOnce(&Means) -> R,
    ) -> R {
        with_means_barred(sheet, fighter, (cost, &|_| false), run)
    }

    /// What each power costs, by row (`Means::cost`).
    type Cost<'a> = &'a dyn Fn(u16) -> Option<i32>;

    /// `with_means`, the items worn barring the powers `barred` says.
    fn with_means_barred<R>(
        sheet: Sheet,
        fighter: &Fighter,
        (cost, barred): (Cost<'_>, &dyn Fn(u16) -> bool),
        run: impl FnOnce(&Means) -> R,
    ) -> R {
        Rules::with(None, false, |rules| {
            run(&Means {
                sheet,
                fighter,
                me: SimId(ME),
                cost,
                barred,
                choices: force::choices(&sheet.powers, None),
                assignable: bar::assignable(&sheet, rules),
                now_ms: 0,
            })
        })
    }

    /// Pull has no `spells.2da` row of its own and uses Push's. The HUD dims
    /// Pull when the character has fewer Force points than Push costs. With
    /// KOTOR's install, the wheel and the bar setup screen name it "Force
    /// Pull" and show Push's description. Without the install, only the
    /// dimming is checked.
    fn pull_reads_push() -> Result<String, String> {
        let (mut sheet, fighter) = soldier();
        sheet.powers.insert(PUSH);
        let push_cost = |row| (row == PUSH).then_some(50);
        let dimmed = with_means(sheet, &fighter, &push_cost, |means| {
            !payable(Content::Power(force::PULL), means)
        });
        let discovery = crate::discover();
        let Ok(assets) = ui::assets::GuiAssets::open(&discovery, vfs::Game::Kotor1) else {
            return check(
                dimmed,
                format!("dimmed at 0 of 50 Force points {dimmed}, k1: no install"),
            );
        };
        let stack = ui::screen::Stack::new(ui::screens::open);
        let mut gui = crate::ui_view::Ui::new(stack, Some(assets), Default::default(), true);
        let chosen = crate::hud::Chosen::default();
        let name = crate::hud::bar_name(
            &mut gui,
            Content::Power(force::PULL),
            &sheet,
            (&chosen, None),
        );
        let told = |gui: &mut crate::ui_view::Ui, row| {
            crate::hud::bar_description(gui, Content::Power(row), &sheet, &chosen)
        };
        let (pull, push) = (told(&mut gui, force::PULL), told(&mut gui, PUSH));
        check(
            dimmed && name == crate::hud::PULL_NAME && !pull.is_empty() && pull == push,
            format!(
                "dimmed at 0 of 50 Force points {dimmed}, name {:?}, description {:?}, Push's the same {}",
                String::from_utf8_lossy(&name),
                String::from_utf8_lossy(&pull),
                pull == push,
            ),
        )
    }

    /// The bar setup screen's description of Stun and of Stasis goes on
    /// with the rulebook's lines, as the abilities page's does. The line on
    /// the longest a stun lasts is among them, so the player reads the time
    /// the game plays by beside the time KOTOR's own description gives
    /// (Discord report, 2026-11-17). Needs KOTOR's install.
    fn rulebook_in_descriptions() -> Result<String, String> {
        use sim::combat::spells::{FORCE_POWER_STASIS as STASIS, FORCE_POWER_STUN as STUN};
        let discovery = crate::discover();
        let Ok(assets) = ui::assets::GuiAssets::open(&discovery, vfs::Game::Kotor1) else {
            return check(true, "k1: no install".into());
        };
        let longest = rules::applying("spell:FORCE_POWER_STUN")
            .find(|entry| entry.path == "force.crowd.hold_ms")
            .map(|entry| {
                entry
                    .summary
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .ok_or("the rulebook has no maximum stun time for Stun")?;
        let stack = ui::screen::Stack::new(ui::screens::open);
        let mut gui = crate::ui_view::Ui::new(stack, Some(assets), Default::default(), true);
        let (sheet, _) = soldier();
        let chosen = crate::hud::Chosen::default();
        let quotes = [STUN, STASIS].map(|row| {
            let told = crate::hud::bar_description(&mut gui, Content::Power(row), &sheet, &chosen);
            told.windows(longest.len())
                .any(|part| part == longest.as_bytes())
        });
        check(
            quotes == [true, true],
            format!("Stun and Stasis quote {longest:?}: {quotes:?}"),
        )
    }

    const FOE: u32 = 7;
    const ME: u32 = 3;
    const HAND: u8 = 0;

    /// A slot's case: its name, what it holds, whether the force button can
    /// cast it, its key pressed and held, the target, and what it does.
    type Case = (String, Content, bool, bool, bool, Option<u32>, BarUse);

    fn uses(force: Option<u16>, buttons: u16, actions: Vec<Action>) -> BarUse {
        BarUse {
            force,
            buttons,
            actions,
            held: None,
            // A mine being laid keeps the character still.
            still: buttons & buttons::LAY_MINE != 0,
        }
    }

    /// What a slot holding `content` does, with JA's powers (no game data).
    fn used(
        content: Content,
        listed: bool,
        usable: bool,
        keys: (bool, bool),
        foe: Option<u32>,
    ) -> BarUse {
        let choices: &[u16] = match content {
            Content::Power(row) if listed => &[row],
            _ => &[],
        };
        Rules::with(None, false, |rules| {
            let target = cast_target(rules, content, foe, ME);
            slot_use(content, choices, usable, keys.0, keys.1, HAND, target)
        })
    }

    fn slot_uses() -> Result<String, String> {
        let none = BarUse::default();
        let cast = |row, target| uses(None, 0, vec![Action::Cast { row, target }]);
        let feat = Content::Feat(FeatLine::RapidShot);
        let mut cases: Vec<Case> = vec![
            (
                "a listed power held".into(),
                Content::Power(PUSH),
                true,
                false,
                true,
                None,
                uses(Some(PUSH), buttons::FORCE_POWER, vec![]),
            ),
            (
                "a listed power pressed".into(),
                Content::Power(PUSH),
                true,
                true,
                true,
                None,
                uses(Some(PUSH), buttons::FORCE_POWER, vec![]),
            ),
            (
                "a listed power let go".into(),
                Content::Power(PUSH),
                true,
                false,
                false,
                None,
                none.clone(),
            ),
            (
                "a hostile power pressed".into(),
                Content::Power(PUSH),
                false,
                true,
                true,
                Some(FOE),
                cast(PUSH, FOE),
            ),
            (
                "a hostile power held on".into(),
                Content::Power(PUSH),
                false,
                false,
                true,
                Some(FOE),
                none.clone(),
            ),
            (
                "a hostile power with no target".into(),
                Content::Power(PUSH),
                false,
                true,
                true,
                None,
                none.clone(),
            ),
            (
                "a friendly power pressed".into(),
                Content::Power(HEAL),
                false,
                true,
                true,
                Some(FOE),
                cast(HEAL, ME),
            ),
            (
                "mines pressed".into(),
                Content::Mine,
                false,
                true,
                true,
                None,
                uses(None, buttons::LAY_MINE, vec![]),
            ),
            (
                "stealth pressed".into(),
                Content::Stealth,
                false,
                true,
                true,
                None,
                uses(None, buttons::STEALTH, vec![]),
            ),
            (
                "stealth let go".into(),
                Content::Stealth,
                false,
                false,
                false,
                None,
                none.clone(),
            ),
            (
                "a feat attack pressed".into(),
                feat,
                false,
                true,
                true,
                None,
                uses(
                    None,
                    0,
                    vec![Action::Feat {
                        hand: HAND,
                        line: FeatLine::RapidShot,
                    }],
                ),
            ),
            (
                "a feat attack held on".into(),
                feat,
                false,
                false,
                true,
                None,
                none.clone(),
            ),
            (
                "grenades pressed".into(),
                Content::Item(Shelf::Grenade),
                false,
                true,
                true,
                None,
                uses(None, 0, vec![Action::Grenade(GrenadeOrder::Toggle)]),
            ),
            (
                "grenades held on".into(),
                Content::Item(Shelf::Grenade),
                false,
                false,
                true,
                None,
                none.clone(),
            ),
        ];
        // Grenades are readied by their press (the cases above). Every
        // other kind of item holds its button down while the key is held.
        cases.extend(
            Shelf::ALL
                .into_iter()
                .filter(|shelf| *shelf != Shelf::Grenade)
                .map(|shelf| {
                    (
                        format!("{shelf:?} held"),
                        Content::Item(shelf),
                        false,
                        false,
                        true,
                        None,
                        uses(None, shelf.button(), vec![]),
                    )
                }),
        );
        let wrong: Vec<String> = cases
            .iter()
            .filter(|(_, content, listed, pressed, held, foe, want)| {
                used(*content, *listed, true, (*pressed, *held), *foe) != *want
            })
            .map(|(name, content, listed, pressed, held, foe, _)| {
                format!(
                    "{name}: {:?}",
                    used(*content, *listed, true, (*pressed, *held), *foe)
                )
            })
            .collect();
        check(
            wrong.is_empty(),
            format!("{} cases, wrong {wrong:?}", cases.len()),
        )
    }

    /// Every kind of content.
    const CONTENTS: [(Content, bool); 9] = [
        (Content::Power(PUSH), true),
        (Content::Power(HEAL), false),
        (Content::Item(Shelf::Medpac), false),
        (Content::Item(Shelf::Stim), false),
        (Content::Item(Shelf::Shield), false),
        (Content::Item(Shelf::Grenade), false),
        (Content::Mine, false),
        (Content::Stealth, false),
        (Content::Feat(FeatLine::PowerAttack), false),
    ];

    fn unusable() -> Result<String, String> {
        let wrong: Vec<String> = CONTENTS
            .iter()
            .map(|(content, listed)| {
                (
                    content,
                    used(*content, *listed, false, (true, true), Some(FOE)),
                )
            })
            .filter(|(_, used)| *used != BarUse::default())
            .map(|(content, used)| format!("{content:?} {used:?}"))
            .collect();
        check(
            wrong.is_empty(),
            format!("{} kinds, wrong {wrong:?}", CONTENTS.len()),
        )
    }

    /// Slots 1 and 2 held in that order use slot 1 until it is let go, and
    /// then slot 2. Slot 2 held first keeps it when slot 1 is pressed.
    fn first_held_wins() -> Result<String, String> {
        let held = |slots: &[usize]| std::array::from_fn(|slot| slots.contains(&slot));
        let run = |frames: &[&[usize]]| {
            let mut was = None;
            frames
                .iter()
                .map(|slots| {
                    was = in_use(was, held(slots));
                    was
                })
                .collect::<Vec<_>>()
        };
        let in_order = run(&[&[0], &[0, 1], &[1], &[]]);
        let second_first = run(&[&[1], &[0, 1], &[0]]);
        check(
            in_order == [Some(0), Some(0), Some(1), None]
                && second_first == [Some(1), Some(1), Some(0)],
            format!("slot 1 then 2 {in_order:?}, slot 2 then 1 {second_first:?}"),
        )
    }

    /// A soldier with Power Blast and a blaster pistol, as `Sheet::dev_soldier`
    /// holds it.
    fn soldier() -> (Sheet, Fighter) {
        let mut sheet = Sheet::dev_soldier();
        sheet.feats = sheet.feats.with(feats::POWER_BLAST[0]);
        (sheet, Fighter::fresh(10, 0))
    }

    /// The pistol's own line shows as on before any choice, another line
    /// does not. Stealth mode shows as on while the character is in
    /// stealth, and the grenades while a grenade is readied.
    fn lit_slots() -> Result<String, String> {
        let (sheet, fighter) = soldier();
        let mut hidden = fighter;
        hidden.covert.stealth = true;
        let mut readied = fighter;
        readied.pack.readied = Readied::On;
        let grenades = Content::Item(Shelf::Grenade);
        let seen = [
            lit(Content::Feat(FeatLine::PowerBlast), &sheet, &fighter),
            lit(Content::Feat(FeatLine::RapidShot), &sheet, &fighter),
            lit(Content::Stealth, &sheet, &fighter),
            lit(Content::Stealth, &sheet, &hidden),
            lit(Content::Mine, &sheet, &fighter),
            lit(grenades, &sheet, &fighter),
            lit(grenades, &sheet, &readied),
        ];
        check(
            seen == [true, false, false, true, false, false, true],
            format!(
                "Power Blast, Rapid Shot, stealth off, stealth on, mines, grenades, \
                 a grenade readied: {seen:?}"
            ),
        )
    }

    /// The soldier, with none of its items left, can use its pistol's feat
    /// attack. It cannot use a melee feat attack, a power it does not know,
    /// mines without a kit, stealth mode without a stealth field generator,
    /// or medpacs.
    /// A power the items worn bar (issue 428) is not usable, so the HUD and
    /// the power wheel grey it, and pressing its slot does nothing. The
    /// same power is usable without them.
    fn barred_powers() -> Result<String, String> {
        let (mut sheet, fighter) = soldier();
        sheet.powers.insert(PUSH);
        let seen = |barred: &dyn Fn(u16) -> bool| {
            with_means_barred(sheet, &fighter, (&|_| None, barred), |means| {
                let usable = usable(Content::Power(PUSH), means);
                let used = slot_use(
                    Content::Power(PUSH),
                    &means.choices,
                    usable,
                    true,
                    true,
                    0,
                    Some(ME),
                );
                (usable, used.buttons)
            })
        };
        let (armoured, bare) = (seen(&|row| row == PUSH), seen(&|_| false));
        check(
            armoured == (false, 0) && bare.0 && bare.1 == buttons::FORCE_POWER,
            format!("in armour (usable, buttons) {armoured:?}; without {bare:?}"),
        )
    }

    fn usable_slots() -> Result<String, String> {
        let (mut sheet, fighter) = soldier();
        sheet.items = std::array::from_fn(|_| None);
        let seen = with_means(sheet, &fighter, &|_| None, |means| {
            [
                Content::Feat(FeatLine::PowerBlast),
                Content::Feat(FeatLine::PowerAttack),
                Content::Power(PUSH),
                Content::Mine,
                Content::Stealth,
                Content::Item(Shelf::Medpac),
            ]
            .map(|content| usable(content, means))
        });
        check(
            seen == [true, false, false, false, false, false],
            format!("Power Blast, Power Attack, Push, mines, stealth, medpacs: {seen:?}"),
        )
    }
}
