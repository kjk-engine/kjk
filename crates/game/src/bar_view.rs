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
//! up, or while the power wheel's key is held. A seat that flies the
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
    /// Where the character stands (metres), for the Force rating of TSL's
    /// rooms (`ModuleData::power_cost`).
    pub at: [f32; 3],
    pub module: Option<&'a sim::module::ModuleData>,
    /// The powers the force button can cast (`force::choices`).
    pub choices: Vec<u16>,
    /// What the character can put on the bar now (`bar::assignable`).
    pub assignable: Vec<Content>,
    pub now_ms: u64,
}

impl Means<'_> {
    /// Whether the character has the Force points that power `row` costs
    /// now. A power whose cost the module does not list is left to the sim.
    fn affords(&self, row: u16) -> bool {
        let cost = self
            .module
            .and_then(|module| module.power_cost(row, &self.sheet, true, self.at));
        cost.is_none_or(|cost| {
            sim::combat::effects::can_pay(
                &self.fighter.effects.lasting,
                (self.fighter.fp, self.fighter.hp),
                cost * 100,
                self.now_ms,
            )
        })
    }
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
    let (body, sheet) = crate::cast::body_sheet(snapshot, seat, level)?;
    let fighter = body.fighter.as_ref()?;
    let library = level.scripts().map(|library| &**library);
    let mines =
        library.is_some_and(|library| sim::traps::party_has_kit(&snapshot.goods, library.items()));
    Some(Rules::with(library, mines, |rules| {
        let means = Means {
            sheet,
            fighter,
            me: body.id,
            at: body.position,
            module: level.module().map(|module| &**module),
            choices: force::choices(&sheet.powers, library),
            assignable: bar::assignable(&sheet, rules),
            now_ms: snapshot.time_ms,
        };
        run(&means, rules)
    }))
}

/// Whether the character can use `content` now. A power must be one it can
/// cast and pay for. An item kind must have an item left that the sim's
/// key would use. A feat attack must be known and fit the weapon in hand.
/// Mines need a mine kit and a rank in Demolitions, and stealth mode needs
/// a stealth field generator and a rank in Stealth (`bar::assignable`).
pub fn usable(content: Content, means: &Means) -> bool {
    let (sheet, fighter) = (&means.sheet, means.fighter);
    match content {
        Content::Power(row) => means.assignable.contains(&content) && means.affords(row),
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
    targeted: Option<Res<crate::target_view::Targeted>>,
    mut outgoing: Option<ResMut<Outgoing>>,
    mut bar_use: ResMut<BarUse>,
    mut was: Local<Option<usize>>,
) {
    *bar_use = BarUse::default();
    let busy = crate::pick_view::busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    let Some(actions) = actions.filter(|actions| !busy && !actions.held(Key::Wheel)) else {
        *was = None;
        return;
    };
    let taken = step(
        &mut was,
        BAR.map(|key| actions.held(key)),
        BAR.map(|key| actions.pressed(key)),
    );
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
    let foe = targeted
        .and_then(|targeted| targeted.object)
        .and_then(|number| sim::target::entity(snapshot, number))
        .map(|entity| entity.id.0);
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
    use sim::combat::bar::FeatLine;
    use sim::combat::items::Shelf;
    use sim::combat::sheet::feats;
    use sim::combat::spells::{FORCE_POWER_FORCE_PUSH as PUSH, FORCE_POWER_HEAL as HEAL};
    use sim::probe::{Checks, check};

    pub fn probes() -> Checks {
        vec![
            ("slot use", slot_uses()),
            ("unusable slots do nothing", unusable()),
            ("first held wins", first_held_wins()),
            ("lit slots", lit_slots()),
            ("usable slots", usable_slots()),
            ("reply keys use no slot", reply_keys()),
        ]
    }

    /// A frame of slot 3's key (Bar3, the number 3 by default): its name,
    /// whether a conversation is up, whether the key is down and whether it
    /// went down this frame, and whether the slot is used.
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

    /// The keys pass the screens' gate (`pad::gate_keys`) and then the bar
    /// (`step`, `slot_use`). Slot 3 holds medpacs, then a friendly power
    /// the force button cannot cast. The number that picks a reply uses
    /// neither, nor does it while still held as play resumes. Let go and
    /// pressed again, it uses the slot.
    fn reply_keys() -> Result<String, String> {
        let key = crate::pad::bit(BAR[2]);
        let mut wrong = Vec::new();
        for content in [Content::Item(Shelf::Medpac), Content::Power(HEAL)] {
            let (mut kept, mut was) = (0, None);
            for (name, talk, down, went, uses) in REPLY_FRAMES {
                let bits = |on: bool| if on { key } else { 0 };
                let (held, pressed) =
                    crate::pad::gate_keys(talk, &mut kept, bits(down), bits(went));
                let slots = |bits: u64| BAR.map(|key| bits & crate::pad::bit(key) != 0);
                let used = step(&mut was, slots(held), slots(pressed))
                    .filter(|(slot, _)| *slot == 2)
                    .map_or(BarUse::default(), |(_, pressed)| {
                        used(content, false, true, (pressed, true), Some(FOE))
                    });
                if (used != BarUse::default()) != uses {
                    wrong.push(format!("{content:?}, {name}: {used:?}"));
                }
            }
        }
        check(
            wrong.is_empty(),
            format!(
                "{} frames for medpacs and Heal, wrong {wrong:?}",
                REPLY_FRAMES.len()
            ),
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
                at: [0.0; 3],
                module: None,
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
