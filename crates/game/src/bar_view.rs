//! The action bar from the player's side (`sim::combat::bar`). Each of the
//! six slot keys (`settings::BAR`) uses what its slot holds:
//!
//! - A Force power that the force button can cast is cast with that button
//!   while the key is held (`Command::force`). Any other power is cast on
//!   the press (`Action::Cast`), at the target for a power used on an enemy
//!   and at the character itself for any other power.
//! - An item kind is used with its button while the key is held. The item
//!   picker (`pick_view`) treats the key as that kind's key, so a tap uses
//!   the item and holding the key lets the player step through the kinds.
//! - Mines lay a mine, and stealth mode turns stealth on or off, as the key
//!   goes down.
//! - A feat attack is chosen for the weapon in hand on the press
//!   (`Action::Feat`).
//!
//! Only one slot is used at a time. The first key held wins until it is
//! let go. Nothing is used while a screen, a conversation or a movie is
//! up, or while the power wheel is open. A seat that flies the
//! remote droid has no bar: slot 1's key fires the droid's stim dart.

use bevy::prelude::*;
use sim::action::Action;
use sim::buttons;
use sim::combat::bar::{self, Content, Rules, SLOTS};
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
    /// The powers the force button can cast (`force::choices`).
    pub choices: Vec<u16>,
    /// What the character can put on the bar now (`bar::assignable`).
    pub assignable: Vec<Content>,
    pub now_ms: u64,
}

/// Whether the character has the Force points to start the power `content`
/// holds, for the HUD to dim a slot it cannot pay for. Any other content,
/// and a power whose cost the module does not list, counts as paid for.
///
/// This is never a reason to hold back a slot's press or hold. The sim
/// refuses a cast it cannot pay for. A channelled power held on pays only
/// each step's smaller cost (`force::channel_cost`), and a running toggle
/// such as Force Speed turns off with no payment, so the client's check of
/// the whole cost would stop both too early.
pub fn payable(content: Content, means: &Means) -> bool {
    let Content::Power(row) = content else {
        return true;
    };
    (means.cost)(row).is_none_or(|cost| {
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
    Some(Rules::with(library, mines, |rules| {
        let means = Means {
            sheet,
            fighter,
            me: body.id,
            cost: &cost,
            choices: force::choices(&sheet.powers, library),
            assignable: bar::assignable(&sheet, rules),
            now_ms: snapshot.time_ms,
        };
        run(&means, rules)
    }))
}

/// Whether the character can use `content` now. A power must be one it can
/// cast. Its cost is left to the sim (`payable`). An item kind must have an item left that the sim's
/// key would use. A feat attack must be known and fit the weapon in hand.
/// Mines need a mine kit and a rank in Demolitions, and stealth mode needs
/// a stealth field generator and a rank in Stealth (`bar::assignable`).
pub fn usable(content: Content, means: &Means) -> bool {
    let (sheet, fighter) = (&means.sheet, means.fighter);
    match content {
        Content::Power(_) => means.assignable.contains(&content),
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
/// weapon in hand uses now (a gun's first known line too), or stealth mode
/// while the character is in stealth.
pub fn lit(content: Content, sheet: &Sheet, fighter: &Fighter) -> bool {
    match content {
        Content::Feat(line) => Picks::in_use(sheet, fighter.hand) == Some(line),
        Content::Stealth => fighter.covert.stealth,
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
        Content::Item(shelf) => out.buttons = shelf.button(),
        // The sim lays a mine and turns stealth on or off as its button
        // goes down (`world::covert`), so the button is held with the key.
        Content::Mine => out.buttons = buttons::LAY_MINE,
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
    (targeted, wheel): (
        Option<Res<crate::target_view::Targeted>>,
        Option<Res<crate::wheel_view::Wheel>>,
    ),
    mut outgoing: Option<ResMut<Outgoing>>,
    mut bar_use: ResMut<BarUse>,
    (mut was, mut kept): (Local<Option<usize>>, Local<u64>),
) {
    *bar_use = BarUse::default();
    let Some(actions) = actions else {
        *was = None;
        return;
    };
    let busy = crate::pick_view::busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    let closed = busy || wheel.is_some_and(|wheel| wheel.open);
    let (held, pressed) = held_back(closed, &mut kept, &actions);
    let taken = step(&mut was, held, pressed);
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
    let used = with_bar(snapshot, own.0, &swap.level, |means, rules| {
        let content = means.sheet.bar.slots()[slot]?;
        let target = cast_target(rules, content, foe, means.me.0);
        let usable = usable(content, means);
        let hand = means.fighter.hand;
        Some(slot_use(
            content,
            &means.choices,
            usable,
            pressed,
            true,
            hand,
            target,
        ))
    });
    let Some(used) = used.flatten() else {
        return;
    };
    if let Some(outgoing) = outgoing.as_mut() {
        outgoing.0.extend(used.actions.iter().cloned());
    }
    *bar_use = BarUse {
        held: Some(slot),
        ..used
    };
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
            ("reply keys use no slot", reply_keys()),
            ("conversation keys", talk_keys()),
            ("wheel keys use no slot", wheel_keys()),
            ("held powers ignore the start cost", held_powers()),
        ]
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

    const TALK_CASES: [TalkCase; 7] = [
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
        let cost = |_| Some(50);
        let wrong: Vec<String> = Rules::with(None, false, |rules| {
            let means = Means {
                sheet,
                fighter: &fighter,
                me: SimId(ME),
                cost: &cost,
                choices: force::choices(&sheet.powers, None),
                assignable: bar::assignable(&sheet, rules),
                now_ms: 0,
            };
            cases
                .iter()
                .filter_map(|(name, row, pressed)| {
                    let content = Content::Power(*row);
                    let usable = usable(content, &means);
                    let used =
                        slot_use(content, &means.choices, usable, *pressed, true, HAND, None);
                    let want = uses(Some(*row), buttons::FORCE_POWER, vec![]);
                    (used != want || payable(content, &means)).then(|| format!("{name}: {used:?}"))
                })
                .collect()
        });
        check(
            wrong.is_empty(),
            format!("Force points 0 of 50, wrong {wrong:?}"),
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
        ];
        cases.extend(Shelf::ALL.map(|shelf| {
            let name = format!("{shelf:?} held");
            (
                name,
                Content::Item(shelf),
                false,
                false,
                true,
                None,
                uses(None, shelf.button(), vec![]),
            )
        }));
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
    /// does not, and stealth mode shows as on while the character is in
    /// stealth.
    fn lit_slots() -> Result<String, String> {
        let (sheet, fighter) = soldier();
        let mut hidden = fighter;
        hidden.covert.stealth = true;
        let seen = [
            lit(Content::Feat(FeatLine::PowerBlast), &sheet, &fighter),
            lit(Content::Feat(FeatLine::RapidShot), &sheet, &fighter),
            lit(Content::Stealth, &sheet, &fighter),
            lit(Content::Stealth, &sheet, &hidden),
            lit(Content::Mine, &sheet, &fighter),
        ];
        check(
            seen == [true, false, false, true, false],
            format!("Power Blast, Rapid Shot, stealth off, stealth on, mines: {seen:?}"),
        )
    }

    /// The soldier can use its pistol's feat attack, but not a melee feat
    /// attack, a power it does not know, mines without a kit, or stealth
    /// mode without a stealth field generator.
    fn usable_slots() -> Result<String, String> {
        let (sheet, fighter) = soldier();
        let seen = Rules::with(None, false, |rules| {
            let means = Means {
                sheet,
                fighter: &fighter,
                me: SimId(ME),
                cost: &|_| None,
                choices: force::choices(&sheet.powers, None),
                assignable: bar::assignable(&sheet, rules),
                now_ms: 0,
            };
            [
                Content::Feat(FeatLine::PowerBlast),
                Content::Feat(FeatLine::PowerAttack),
                Content::Power(PUSH),
                Content::Mine,
                Content::Stealth,
            ]
            .map(|content| usable(content, &means))
        });
        check(
            seen == [true, false, false, false, false],
            format!("Power Blast, Power Attack, Push, mines, stealth: {seen:?}"),
        )
    }
}
