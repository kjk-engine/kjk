//! `kjk combat duel`: I24 part 2's evidence without a window. Two players
//! on a floor (friendly fire full, so they may hurt and lock each other),
//! JA's skeleton and sabers from the install, driven by scripted keys as a
//! player's hands would drive them, stepped through `sim::step` on the
//! authority; each scene checked against the rules
//! (`docs/COMBAT.md`, C5, C6, C22):
//!
//! - **cuts**: each style swings at a defenceless dummy; every hit's
//!   location, the share of its swing's budget each contact delivered and
//!   the damage it came to, checked to the hundredth against the C5 budget
//!   (a fixed roll, so the pipeline's arithmetic is visible);
//! - **feat attacks**: each style's KOTOR feat line (C8) shaping the
//!   saber a swing carries, at each tier;
//! - **blocks**: the same swings at defence 0 to 3 from the front and from
//!   behind (C6's arcs), and a proficient vibroblade's (C9);
//! - **bolts**: a rifle at a lit saber at each defence level, and with a
//!   deflect property: blocks, absorbs, deflections, reflections;
//! - **bolts at a blade**: a gun aimed at a side-on holder's blade: a
//!   lightsaber turns them (at defence 0 too), an ion bolt bursts on it, a
//!   vibroblade neither turns nor stops any;
//! - **clashes**: two attackers swinging at each other: the tree's outcomes;
//! - **locks**: a lock won by pressing and lost by not;
//! - **throws**: out, back, caught; a hit; knocked to the ground by a
//!   better defender and called back;
//! - **kicks**: the staff's kick: the hit, the toss, the knockdown and the
//!   kick tweak's exception;
//! - **roll stab**: the fast style's stab from a forward roll cuts a dummy;
//! - **cost**: end_m01aa with 2, 8 and 26 sword users fighting, a tick's
//!   time with the blades and without them (release build).
//!
//! Writes `facts.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use sim::combat::damage::{self};
use sim::combat::fighter::{Event, Location, Outcome};
use sim::combat::saber::{Kit, Move, Style, blades, throw::Flight};
use sim::combat::sheet::{Sheet, Weapon, damage_types, factions, feats};
use sim::combat::{Fighter, mind};
use sim::module::ObjectKind;
use sim::motion::{self, AnimTable, MotionState};
use sim::{
    Command, EntityState, Kind, Level, Recorder, Recording, Seat, SimWorld, Snapshot, StepReason,
    TickInput, TickRate, buttons, step,
};
use vfs::Discovery;

use crate::by_hand::with_defence;
use crate::saber_verbs::{ALT, ATTACK, Report, STYLES, jedi, saber, weapons_for};
use crate::scenes::{self, Stage};
use crate::stage::ring;


/// Bouts kept to be watched (`kjk play --replay`): each one's name and
/// recording, written beside the facts.
pub(crate) type Bouts = Vec<(String, Recording)>;

/// What a seat does in a tick.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Keys {
    forward: i8,
    right: i8,
    up: i8,
    buttons: u16,
    /// Degrees turned from facing the other (180: back to it).
    turn: f32,
}

impl Keys {
    pub(crate) fn press(buttons: u16) -> Keys {
        Keys {
            buttons,
            ..Keys::default()
        }
    }

    pub(crate) fn with(self, forward: i8, right: i8) -> Keys {
        Keys {
            forward,
            right,
            ..self
        }
    }

    pub(crate) fn turned(self, turn: f32) -> Keys {
        Keys { turn, ..self }
    }
}

/// Two players on the ring, the bout recorded.
pub(crate) struct Bout {
    world: SimWorld,
    pub(crate) last: Snapshot,
    pub(crate) rate: TickRate,
    level: Level,
    /// Events seen, by (entity, sequence).
    seen: BTreeSet<(u32, u16)>,
    /// How far behind each seat's player sees the world (ms), sent with its
    /// commands (I25, C18 and C19).
    pub(crate) lags: [u16; 2],
    recorder: Recorder,
}

impl Bout {
    /// Seat 0 with `sheets[0]` at the origin, seat 1 `apart` units along +X,
    /// facing each other.
    pub(crate) fn new(level: &Level, sheets: [Sheet; 2], apart: f32, seed: u64) -> Bout {
        let config = sim::WorldConfig {
            seed,
            seats: 2,
            ..sim::WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level.clone()).expect("two seats on the ring");
        let mut start = world.snapshot();
        for (seat, sheet) in sheets.into_iter().enumerate() {
            let entity = start
                .entities
                .iter_mut()
                .find(|e| e.kind == Kind::Avatar(Seat(seat as u8)))
                .expect("a seat");
            let origin = [apart * seat as f32, 0.0, 0.125 - motion::SOLE_Z];
            let yaw = if seat == 0 { 0.0 } else { 180.0 };
            entity.player = Some(MotionState::standing(origin, yaw));
            entity.sheet = Some(sheet);
            entity.fighter = Some(Fighter::of(&sheet));
        }
        let world = SimWorld::from_snapshot(&start, level.clone()).expect("the bout starts");
        let rate = TickRate::default();
        Bout {
            world,
            recorder: Recorder::new(rate.hz(), start.clone(), level.id(), 100),
            last: start,
            rate,
            level: level.clone(),
            seen: BTreeSet::new(),
            lags: [0; 2],
        }
    }

    pub(crate) fn seat(&self, seat: u8) -> &EntityState {
        seat_in(&self.last, seat)
    }

    pub(crate) fn ps(&self, seat: u8) -> MotionState {
        self.seat(seat).player.expect("a player")
    }

    pub(crate) fn fighter(&self, seat: u8) -> Fighter {
        self.seat(seat).fighter.expect("a fighter")
    }

    pub(crate) fn now(&self) -> u64 {
        self.last.time_ms
    }

    /// `seat`'s command for `keys`, looking at the chest of `them` (the
    /// other as this seat's player sees it).
    pub(crate) fn command(&self, seat: u8, keys: Keys, them: &MotionState) -> Command {
        let me = self.ps(seat);
        // A gun aims its crosshair (where its shots go); a blade the eye.
        let gun = self
            .seat(seat)
            .sheet
            .is_some_and(|sheet| sheet.main_weapon().delivery.ranged());
        let from = if gun { mind::crosshair(&me) } else { mind::eye(&me) };
        let (pitch, yaw) = mind::aim(from, mind::chest(them));
        Command {
            forward: keys.forward,
            right: keys.right,
            up: keys.up,
            buttons: keys.buttons,
            ..mind::looking(&me, pitch, yaw + keys.turn)
        }
    }

    /// One tick of both seats' commands, each with its view lag; recorded.
    pub(crate) fn advance(&mut self, commands: [Command; 2]) {
        let mut input = TickInput::default();
        for (seat, command) in (0..2u8).zip(commands) {
            input.insert(Seat(seat), command);
            input.set_view_lag(Seat(seat), self.lags[usize::from(seat)]);
        }
        let tick = self.last.tick + 1;
        let msec = self.rate.msec(tick);
        self.last = step(&mut self.world, tick, &input, msec, StepReason::Authority);
        self.recorder.record(msec, &input, &self.last);
    }

    /// One tick of both seats' keys, each looking at the other's chest.
    fn step(&mut self, keys: [Keys; 2]) {
        let commands =
            [0u8, 1].map(|seat| self.command(seat, keys[usize::from(seat)], &self.ps(1 - seat)));
        self.advance(commands);
    }

    /// The bout's recording to where it is.
    pub(crate) fn finish(self) -> Recording {
        self.recorder.finish(&self.last)
    }

    /// Whether the bout's recording replays to where it is.
    pub(crate) fn replays(self) -> bool {
        let recording = self.recorder.finish(&self.last);
        recording
            .play(&self.level)
            .is_ok_and(|end| end.hash() == recording.end)
    }

    pub(crate) fn hold(&mut self, ms: u64, keys: [Keys; 2]) {
        let end = self.now() + ms;
        while self.now() < end {
            self.step(keys);
        }
    }

    /// The events recorded since the last call, by the seat they happened
    /// to.
    pub(crate) fn events(&mut self) -> Vec<(u8, Event)> {
        let mut out = Vec::new();
        for entity in &self.last.entities {
            let (Kind::Avatar(seat), Some(fighter)) = (entity.kind, entity.fighter) else {
                continue;
            };
            let mut new: Vec<Event> = fighter
                .events
                .iter()
                .flatten()
                .filter(|e| self.seen.insert((entity.id.0, e.sequence)))
                .copied()
                .collect();
            new.sort_by_key(|e| e.sequence);
            out.extend(new.into_iter().map(|e| (seat.0, e)));
        }
        out
    }

    /// Presses the style button until `seat` fights in `style`.
    fn take_style(&mut self, seat: u8, style: Style) {
        for _ in 0..4 {
            if self.fighter(seat).saber.style == Some(style) {
                break;
            }
            let mut keys = [Keys::default(); 2];
            keys[usize::from(seat)] = Keys::press(buttons::SABER_STYLE);
            self.step(keys);
            self.step([Keys::default(); 2]);
        }
        self.hold(600, [Keys::default(); 2]);
        self.events();
    }

    /// The bout made again from where it is, with `seat`'s hit points
    /// `hp` (hundredths); its recording starts there.
    fn wound(&mut self, seat: u8, hp: i32) {
        let mut start = self.last.clone();
        let entity = start
            .entities
            .iter_mut()
            .find(|e| e.kind == Kind::Avatar(Seat(seat)))
            .expect("a seat");
        if let Some(fighter) = entity.fighter.as_mut() {
            fighter.hp = hp;
        }
        self.world = SimWorld::from_snapshot(&start, self.level.clone()).expect("the bout goes on");
        self.recorder = Recorder::new(self.rate.hz(), start.clone(), self.level.id(), 100);
        self.last = start;
    }

    /// Lights both sabers (a first press of attack) and lets them settle.
    pub(crate) fn light(&mut self) {
        self.step([Keys::press(ATTACK), Keys::press(ATTACK)]);
        self.hold(1200, [Keys::default(); 2]);
        self.events();
    }
}

/// `seat`'s player in `snapshot`.
pub(crate) fn seat_in(snapshot: &Snapshot, seat: u8) -> &EntityState {
    snapshot.own(Seat(seat)).expect("a seat")
}

/// A lightsaber whose roll is always 10 (no dice, +10; crits ×1): the
/// budget's arithmetic without chance.
fn fixed_saber(row: u16) -> Weapon {
    Weapon {
        dice: 0,
        die: 0,
        damage_bonus: 10,
        crit_mult: 1,
        ..saber(row)
    }
}

/// An attacker of `style` with the fixed saber, AB 20 (the margin at its
/// cap, 95%).
fn swordsman(style: Style) -> Sheet {
    let weapons = weapons_for(style).map(|w| w.map(|w| fixed_saber(w.base_item)));
    let mut sheet = jedi(weapons, 100);
    sheet.bab = 20;
    sheet
}

/// `sheet` behind a Defense no attack bonus reaches: only a natural 20
/// hits (the margin's floor), so its blade turns nearly every blow its
/// block or a clash would turn (`damage::chance`, the owner's balance of
/// 2026-10-15).
fn guarded(mut sheet: Sheet) -> Sheet {
    sheet.defence = i8::MAX;
    sheet
}

/// A dummy that cannot block (no Jedi Defense), many HP.
fn dummy(weapons: [Option<Weapon>; 2]) -> Sheet {
    let mut sheet = with_defence(jedi(weapons, 100), 0);
    sheet.max_hp = 2000;
    sheet
}

/// The hundredths a contact delivering `share` at `cadence` lands for at
/// `location`: the fixed `roll` through the pipeline's steps (C1, C2, C5;
/// AB 20 keeps the margin at its cap under a feat's penalty).
fn expected(roll: i64, share: u16, cadence: u32, location: Location) -> i32 {
    let p = damage::margin(20, 10);
    let centi = (roll * 100 * p / 1000).saturating_mul(damage::pace(cadence)) / 1000;
    let sized = centi.saturating_mul(i64::from(share)) / 1000;
    let located = sized.saturating_mul(location.per_mille()) / 1000;
    located.max(1) as i32
}

/// The styles, in cycling order.
/// The keys a swing takes: attack with the movement keys of each plain
/// attack (rulebook 2.5).
pub(crate) const SWINGS: [(&str, i8, i8); 5] = [
    ("top-down", 127, 0),
    ("left-right", 0, 127),
    ("right-left", 0, -127),
    ("TL-BR", 127, 127),
    ("TR-BL", 127, -127),
];

/// Cuts: each style at a dummy (see the module documentation).
fn cuts(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!(
        "cuts: each style's swings at a dummy that cannot block (fixed roll 10 and the style's feat damage, AB 20)"
    );
    let mut rows = Vec::new();
    let mut exact = (0usize, 0usize);
    let mut over_budget = 0;
    let mut locations: BTreeMap<String, usize> = BTreeMap::new();
    for style in STYLES {
        let sheet = swordsman(style);
        // The fixed roll as the style's feat attack shapes it (C8: Power
        // Attack's damage in the strong style).
        let roll = sheet.weapons[0].map_or(10, |weapon| {
            i64::from(Kit::of(&sheet).swing(style, weapon).damage_bonus)
        });
        let mut bout = Bout::new(level, [sheet, dummy([None, None])], 48.0, 1);
        bout.light();
        bout.take_style(0, style);
        let (mut swings, mut contacts, mut delivered, mut damage) = (0, 0, 0u32, 0i64);
        let mut per_swing: Vec<u32> = Vec::new();
        for (_, forward, right) in SWINGS {
            // Each swing: attack held with its keys, then let go to settle.
            let mut before = bout.fighter(0).swing;
            let end = bout.now() + 2400;
            while bout.now() < end {
                let keys = if bout.now() + 900 < end {
                    Keys::press(ATTACK).with(forward, right)
                } else {
                    Keys::default()
                };
                bout.step([keys, Keys::default()]);
                let swing = bout.fighter(0).swing;
                // What was delivered this tick: an old swing's pending at
                // its end, then the contacts the blade left.
                let mut deliveries: Vec<(u16, u32)> = Vec::new();
                let same = swing.saber_move == before.saber_move && swing.flip == before.flip;
                if !same {
                    if swing.saber_move.attack() {
                        swings += 1;
                    }
                    deliveries.extend(
                        before
                            .contacts
                            .iter()
                            .flatten()
                            .filter_map(|c| c.due_share())
                            .map(|s| (s, before.cadence_ms)),
                    );
                }
                for contact in swing.contacts.iter().flatten() {
                    let was = if same {
                        before
                            .contacts
                            .iter()
                            .flatten()
                            .find(|c| c.victim == contact.victim)
                            .map_or(0, |c| c.delivered)
                    } else {
                        0
                    };
                    if contact.delivered > was {
                        deliveries.push((contact.delivered - was, swing.cadence_ms));
                    }
                    if contact.delivered > 1000 {
                        over_budget += 1;
                    }
                }
                let hits: Vec<Event> = bout
                    .events()
                    .into_iter()
                    .filter(|(seat, e)| *seat == 1 && e.by.is_some())
                    .map(|(_, e)| e)
                    .collect();
                for (event, (share, cadence)) in hits.iter().zip(&deliveries) {
                    per_swing.push(u32::from(*share));
                    contacts += 1;
                    delivered += u32::from(*share);
                    let want = expected(roll, *share, *cadence, event.location);
                    exact.0 += usize::from(event.damage == want);
                    exact.1 += 1;
                    damage += i64::from(event.damage);
                    *locations
                        .entry(format!("{:?}", event.location))
                        .or_insert(0) += 1;
                }
                exact.1 += hits.len().abs_diff(deliveries.len());
                before = swing;
            }
        }
        let mean = if contacts == 0 {
            0.0
        } else {
            f64::from(delivered) / contacts as f64
        };
        outln!(
            "  {:<6} {swings:>3} swings, {contacts:>3} contacts, mean share {mean:>6.1}‰ (max {:>4}‰), {:>7.2} HP (roll {roll})",
            style.name(),
            per_swing.iter().max().copied().unwrap_or(0),
            damage as f64 / 100.0
        );
        report.check(
            format!("{}: the blade cuts the dummy", style.name()),
            contacts > 0,
            format!("{contacts} contacts"),
        );
        rows.push(serde_json::json!({
            "style": style.name(), "swings": swings, "contacts": contacts,
            "mean_share": mean, "shares": per_swing, "damage_hp": damage as f64 / 100.0,
        }));
    }
    outln!("  locations: {locations:?}");
    report.check(
        "every hit lands as the C5 budget says, to the hundredth",
        exact.1 > 0 && exact.0 == exact.1,
        format!("{} of {}", exact.0, exact.1),
    );
    report.check(
        "no body is owed more than one budget a swing",
        over_budget == 0,
        format!("{over_budget} over"),
    );
    serde_json::json!({ "styles": rows, "locations": locations, "exact": exact })
}

/// Feat attacks (C8): a saber swung in each style at each tier of its
/// feat line, as `Kit::swing` shapes it: (damage, to hit, threat range).
fn feat_attacks(report: &mut Report) -> serde_json::Value {
    outln!("feat attacks: a saber (threat 2) as each style's feat line shapes it");
    let base = saber(8);
    let shape = |weapon: Weapon| {
        (
            i32::from(weapon.damage_bonus - base.damage_bonus),
            i32::from(weapon.attack_bonus - base.attack_bonus),
            i32::from(weapon.crit_threat),
        )
    };
    let mut rows = Vec::new();
    for tier in 0..=3u8 {
        let kit = Kit {
            power: tier,
            critical: tier,
            flurry: tier.min(2),
            two_weapon: tier,
            ..Kit::default()
        };
        let shapes: Vec<(Style, (i32, i32, i32))> = STYLES
            .into_iter()
            .map(|style| (style, shape(kit.swing(style, base))))
            .collect();
        outln!(
            "  tier {tier}: {}",
            shapes
                .iter()
                .map(|(style, (damage, hit, threat))| format!(
                    "{} {damage:+} dmg {hit:+} hit threat {threat}",
                    style.name()
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let t = usize::from(tier);
        let threat = i32::from(base.crit_threat.max(1));
        let want = |style: Style| match style {
            Style::Strong if tier > 0 => ([0, 5, 8, 10][t], -3, i32::from(base.crit_threat)),
            Style::Medium if tier > 0 => (0, 0, threat * (t as i32 + 1)),
            Style::Fast if tier > 0 => (0, [0, -4, -2, -2][t], i32::from(base.crit_threat)),
            _ => (0, 0, i32::from(base.crit_threat)),
        };
        let wrong: Vec<&str> = shapes
            .iter()
            .filter(|(style, got)| *got != want(*style))
            .map(|(style, _)| style.name())
            .collect();
        report.check(
            format!("tier {tier}: Power Attack strong, Critical Strike medium, Flurry fast, none dual or staff"),
            wrong.is_empty(),
            format!("wrong: {wrong:?}"),
        );
        rows.push(serde_json::json!({ "tier": tier, "shapes": shapes.iter()
            .map(|(style, s)| serde_json::json!({ "style": style.name(), "damage": s.0, "hit": s.1, "threat": s.2 }))
            .collect::<Vec<_>>() }));
    }
    serde_json::Value::Array(rows)
}

/// Blocks: medium swings at a dummy of each defence level, from the front
/// and from behind; and a vibroblade's block.
fn blocks(level: &Level, report: &mut Report, bouts: &mut Bouts) -> serde_json::Value {
    outln!("blocks: medium swings at defence 0 to 3, the dummy idle, facing and turned away");
    let mut rows = Vec::new();
    let vibroblade = Weapon::plain(
        2,
        1,
        10,
        (2, 2),
        damage_types::SLASHING,
        [
            feats::PROFICIENCY_MELEE,
            feats::WEAPON_FOCUS_MELEE,
            feats::WEAPON_SPEC_MELEE,
        ],
    );
    let defenders: [(String, usize, [Option<Weapon>; 2]); 5] = [
        ("saber, defence 0".into(), 0, weapons_for(Style::Medium)),
        ("saber, defence 1".into(), 1, weapons_for(Style::Medium)),
        ("saber, defence 2".into(), 2, weapons_for(Style::Medium)),
        ("saber, defence 3".into(), 3, weapons_for(Style::Medium)),
        ("vibroblade, proficient".into(), 0, [Some(vibroblade), None]),
    ];
    for (name, defence, weapons) in defenders {
        for turn in [0.0, 180.0] {
            let mut sheet = guarded(with_defence(jedi(weapons, 100), defence));
            sheet.feats.insert(feats::PROFICIENCY_MELEE);
            sheet.max_hp = 2000;
            let mut bout = Bout::new(level, [swordsman(Style::Medium), sheet], 48.0, 2);
            bout.light();
            let (mut hits, mut parried, mut clashed) = (0, 0, 0);
            // Blows that landed with no clash before them in their swing:
            // a clash may crush the parry (Q-0060), a block by state not.
            let mut through = 0;
            for (_, forward, right) in SWINGS {
                let mut crushed = false;
                let end = bout.now() + 1500;
                while bout.now() < end {
                    bout.step([
                        Keys::press(ATTACK).with(forward, right),
                        Keys::default().turned(turn),
                    ]);
                    // A clash marks both; a block by state only the
                    // defender.
                    let events = bout.events();
                    let met = events
                        .iter()
                        .any(|(seat, e)| *seat == 0 && e.result == Outcome::Parried);
                    crushed |= met;
                    for (seat, event) in events {
                        match (seat, event.result) {
                            (1, Outcome::Parried) if met => clashed += 1,
                            (1, Outcome::Parried) => parried += 1,
                            (1, Outcome::Hit | Outcome::Critical) => {
                                hits += 1;
                                through += usize::from(!crushed);
                            }
                            _ => {}
                        }
                    }
                }
                bout.hold(900, [Keys::default(), Keys::default().turned(turn)]);
                bout.events();
            }
            let facing = if turn == 0.0 { "front" } else { "behind" };
            outln!(
                "  {name:<24} {facing:<6} {parried:>3} blocked by state, {clashed:>3} blade to blade, {hits:>3} hit"
            );
            let expect_blocks = (defence > 0 || name.starts_with("vibro")) && turn == 0.0;
            if defence >= 2 && turn == 0.0 {
                report.check(
                    format!(
                        "{name} from the {facing}: holds every blow no clash crushed (QUIRKS.md Q-0060)"
                    ),
                    through == 0 && parried > 0,
                    format!("{parried} blocked, {clashed} clashed, {hits} hit, {through} uncrushed"),
                );
            }
            report.check(
                format!(
                    "{name} from the {facing}: {}",
                    if expect_blocks {
                        "blocks by state"
                    } else {
                        "blocks nothing by state"
                    }
                ),
                if expect_blocks {
                    parried > 0
                } else {
                    parried == 0
                },
                format!("{parried} blocked, {clashed} clashed, {hits} hit"),
            );
            if turn == 0.0 {
                let label = name.replace([',', ' '], "");
                bouts.push((format!("blocks-{label}"), bout.finish()));
            }
            rows.push(serde_json::json!({
                "defender": name, "from": facing, "blocked": parried, "clashed": clashed, "hit": hits,
            }));
        }
    }
    serde_json::json!(rows)
}

/// Bolts: a rifle at a lit saber at each defence level (and level 2 with a
/// deflect property of +5), 10 s each; and at defence 1 backing off, which
/// leaves it level 0: no block by state (QUIRKS.md Q-0047).
fn bolts(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("bolts: a rifle at a lit, idle saber 300 units away, 10 s each");
    let rifle = Weapon::plain(
        20,
        1,
        8,
        (1, 2),
        damage_types::ENERGY,
        [feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
    );
    let mut rows = Vec::new();
    for (defence, deflect, back) in [
        (0, 0, false),
        (1, 0, false),
        (2, 0, false),
        (3, 0, false),
        (2, 5, false),
        (1, 0, true),
    ] {
        let mut shooter = jedi([Some(rifle), None], 0);
        shooter.feats.insert(feats::PROFICIENCY_BLASTER_RIFLE);
        shooter.max_hp = 2000;
        let mut blade = saber(8);
        blade.deflect = deflect;
        let mut sheet = with_defence(jedi([Some(blade), None], 100), defence);
        sheet.max_hp = 2000;
        let mut bout = Bout::new(level, [shooter, sheet], 300.0, 3);
        bout.step([Keys::default(), Keys::press(ATTACK)]);
        bout.hold(1000, [Keys::default(); 2]);
        bout.events();
        let defender_id = bout.seat(1).id;
        let (mut blocked, mut hit, mut returned) = (0usize, 0usize, 0usize);
        let mut turned: BTreeSet<u32> = BTreeSet::new();
        let end = bout.now() + 10_000;
        let holder = Keys::default().with(if back { -127 } else { 0 }, 0);
        while bout.now() < end {
            bout.step([Keys::press(ATTACK), holder]);
            for (seat, event) in bout.events() {
                match (seat, event.result) {
                    (1, Outcome::Deflected) => blocked += 1,
                    (1, Outcome::Hit | Outcome::Critical) => hit += 1,
                    (0, Outcome::Hit | Outcome::Critical) if event.by == Some(defender_id) => {
                        returned += 1
                    }
                    _ => {}
                }
            }
            for entity in &bout.last.entities {
                if entity.missile.is_some_and(|m| m.owner == defender_id) {
                    turned.insert(entity.id.0);
                }
            }
        }
        let sent_on = turned.len();
        let absorbed = blocked.saturating_sub(sent_on);
        let effective = sim::combat::saber::defence::bolt_level(defence as u8, deflect)
            .saturating_sub(u8::from(back));
        let backing = if back { ", backing off" } else { "" };
        outln!(
            "  defence {defence} deflect {deflect:+}{backing} (level {effective}): {blocked:>3} blocked ({absorbed} absorbed, {sent_on} sent on), {hit:>3} hit the body, {returned:>3} came back into the shooter"
        );
        let pass = match effective {
            0 => blocked == 0 && hit > 0,
            1 => blocked > 0 && sent_on == 0,
            2 => blocked > 0 && sent_on > 0 && absorbed == 0,
            _ => blocked > 0 && sent_on > 0 && returned > 0,
        };
        report.check(
            format!("bolts at level {effective} (defence {defence}, deflect {deflect:+}{backing})"),
            pass,
            format!("{blocked} blocked, {absorbed} absorbed, {sent_on} sent on, {returned} returned, {hit} hit"),
        );
        rows.push(serde_json::json!({
            "defence": defence, "deflect": deflect, "back": back, "level": effective,
            "blocked": blocked, "absorbed": absorbed, "sent_on": sent_on,
            "hit": hit, "returned": returned,
        }));
    }
    serde_json::json!(rows)
}

/// Where `seat`'s first blade is now (its middle, units), placed as the
/// sim places it; none unlit.
fn blade_middle(bout: &Bout, seat: u8) -> Option<[f32; 3]> {
    let data = bout.level.anims().skeleton()?;
    let (ps, fighter) = (bout.ps(seat), bout.fighter(seat));
    let kit = Kit::of(bout.seat(seat).sheet.as_ref()?);
    let slot = blades::slots(&kit, &fighter.saber, fighter.hand)[0]?;
    let (pose, legs_yaw) =
        fighter
            .pose
            .pose(data, &ps, (&fighter.saber, fighter.alive()), bout.now())?;
    let placement = blades::placement(&ps, bout.rate.msec(bout.last.tick), legs_yaw);
    let blade = blades::place(data, &pose, bout.now(), &placement, &slot)?;
    Some(std::array::from_fn(|i| {
        (blade.muzzle[i] + blade.tip[i]) / 2.0
    }))
}

/// What became of a gun's bolts at a blade: fired, blocked (absorbed, or
/// sent on), hit the body, flew on past the holder, stopped dead.
#[derive(Debug, Default)]
struct BoltTally {
    fired: usize,
    blocked: usize,
    absorbed: usize,
    sent_on: usize,
    hit: usize,
    passed: usize,
    stopped: usize,
    /// Of those sent on, the hits they made on the shooter.
    returned: usize,
}

/// Bolts at a blade (owner's rule, 2026-10-01): a gun aims at the middle of
/// the holder's blade for 6 s, the holder idle and side-on (no bolt is in
/// its arc, so only the blade itself can turn one). A lightsaber turns what
/// strikes its blade, even at defence 0 (deflected, QUIRKS.md Q-0047), and
/// an ion bolt bursts on it (JA); a vibroblade turns nothing and stops
/// nothing: every bolt goes on to the body or past it.
fn blade_bolts(level: &Level, report: &mut Report, bouts: &mut Bouts) -> serde_json::Value {
    outln!(
        "bolts at a blade: a gun aimed at the blade of an idle, side-on holder 300 units away, 6 s each"
    );
    let gun = |row: u16| {
        Weapon::plain(
            row,
            1,
            8,
            (1, 2),
            damage_types::ENERGY,
            [feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
        )
    };
    let vibroblade = Weapon::plain(
        2,
        1,
        10,
        (2, 2),
        damage_types::SLASHING,
        [feats::PROFICIENCY_MELEE, 0, 0],
    );
    let scenes: [(&str, Weapon, Weapon, usize); 5] = [
        ("rifle, saber at defence 0", gun(20), saber(8), 0),
        ("rifle, saber at defence 1", gun(20), saber(8), 1),
        ("rifle, saber at defence 3", gun(20), saber(8), 3),
        ("ion rifle, saber at defence 3", gun(18), saber(8), 3),
        ("rifle, proficient vibroblade", gun(20), vibroblade, 0),
    ];
    let mut rows = Vec::new();
    for (name, gun, blade, defence) in scenes {
        let mut shooter = jedi([Some(gun), None], 0);
        shooter.feats.insert(feats::PROFICIENCY_BLASTER_RIFLE);
        shooter.max_hp = 2000;
        let mut sheet = with_defence(jedi([Some(blade), None], 100), defence);
        sheet.feats.insert(feats::PROFICIENCY_MELEE);
        sheet.max_hp = 2000;
        let mut bout = Bout::new(level, [shooter, sheet], 300.0, 3);
        bout.step([Keys::default(), Keys::press(ATTACK)]);
        bout.hold(1000, [Keys::default(), Keys::default().turned(90.0)]);
        bout.events();
        let (shooter_id, holder_id) = (bout.seat(0).id, bout.seat(1).id);
        let holder_x = bout.ps(1).origin[0];
        let mut tally = BoltTally::default();
        // Each of the shooter's bolts: whether it went past the holder.
        let mut flying: BTreeMap<u32, bool> = BTreeMap::new();
        let mut turned: BTreeSet<u32> = BTreeSet::new();
        let end = bout.now() + 8000;
        while bout.now() < end {
            let firing = bout.now() + 2000 < end;
            let aim = blade_middle(&bout, 1).unwrap_or_else(|| mind::chest(&bout.ps(1)));
            let me = bout.ps(0);
            // Aimed from the muzzle: its bolt goes there (nothing behind
            // the blade stops the crosshair's line on the open ring).
            let muzzle = match gun.delivery {
                sim::combat::Delivery::Blaster(blaster) => {
                    sim::muzzle(&me, sim::combat::weapons::primary(blaster).muzzle)
                }
                _ => mind::eye(&me),
            };
            let (pitch, yaw) = mind::aim(muzzle, aim);
            let shoot = Command {
                buttons: if firing { ATTACK } else { 0 },
                ..mind::looking(&me, pitch, yaw)
            };
            let hold = bout.command(1, Keys::default().turned(90.0), &me);
            bout.advance([shoot, hold]);
            for (seat, event) in bout.events() {
                match (seat, event.result) {
                    (1, Outcome::Deflected) => tally.blocked += 1,
                    (1, Outcome::Hit | Outcome::Critical) => tally.hit += 1,
                    (0, Outcome::Hit | Outcome::Critical) if event.by == Some(holder_id) => {
                        tally.returned += 1
                    }
                    _ => {}
                }
            }
            let mut now: BTreeMap<u32, bool> = BTreeMap::new();
            for entity in &bout.last.entities {
                let Some(missile) = entity.missile else {
                    continue;
                };
                if missile.owner == holder_id {
                    turned.insert(entity.id.0);
                } else if missile.owner == shooter_id {
                    let past = missile.at(bout.now())[0] > holder_x + 24.0;
                    let was = flying.get(&entity.id.0).copied().unwrap_or(false);
                    now.insert(entity.id.0, was || past);
                }
            }
            // A bolt gone: past the holder, or ended at it.
            for (id, past) in &flying {
                if !now.contains_key(id) {
                    tally.fired += 1;
                    tally.passed += usize::from(*past);
                }
            }
            flying = now;
        }
        tally.fired += flying.len();
        tally.passed += flying.values().filter(|past| **past).count();
        tally.sent_on = turned.len();
        tally.absorbed = tally.blocked.saturating_sub(tally.sent_on);
        tally.stopped = tally
            .fired
            .saturating_sub(tally.blocked + tally.hit + tally.passed);
        outln!("  {name:<32} {tally:?}");
        let t = &tally;
        let (expect, pass) = match (name, defence) {
            (n, _) if n.contains("vibroblade") => (
                "turns and stops nothing: every bolt to the body or past",
                t.fired > 0 && t.blocked == 0 && t.stopped == 0,
            ),
            (n, _) if n.starts_with("ion") => (
                "bursts on the blade (JA): none turned, some stopped",
                t.blocked == 0 && t.stopped > 0,
            ),
            (_, 0) => (
                "deflects what strikes the blade along its view, none back at the shooter",
                t.blocked > 0 && t.sent_on == t.blocked && t.returned == 0,
            ),
            (_, 1) => (
                "absorbs what strikes the blade",
                t.blocked > 0 && t.absorbed == t.blocked,
            ),
            _ => (
                "reflects what strikes the blade back at the shooter",
                t.blocked > 0 && t.sent_on == t.blocked && t.returned > 0,
            ),
        };
        report.check(
            format!("bolts at a blade, {name}: {expect}"),
            pass,
            format!("{t:?}"),
        );
        let label = name.replace(", ", "-").replace(' ', "-");
        bouts.push((format!("bolts-{label}"), bout.finish()));
        rows.push(serde_json::json!({
            "scene": name, "fired": t.fired, "blocked": t.blocked, "absorbed": t.absorbed,
            "sent_on": t.sent_on, "hit": t.hit, "passed": t.passed, "stopped": t.stopped,
            "returned": t.returned,
        }));
    }
    serde_json::json!(rows)
}

/// A lock's break animation for its winner: an old lock's win, or any
/// super-break won.
fn wins(anim: motion::Anim) -> bool {
    use motion::Anim::*;
    matches!(
        anim,
        BOTH_A3_T__B_ | BOTH_K1_S1_T_ | BOTH_CWCIRCLEBREAK | BOTH_CCWCIRCLEBREAK
    ) || anim.name().ends_with("SB_1_W")
        || anim.name().ends_with("B_1_W")
}

/// A medium swordsman whose offence is 1 (proficiency alone): a lock's
/// strength of 2, so its wins rarely super-break.
fn plain_swordsman() -> Sheet {
    let mut sheet = swordsman(Style::Medium);
    for row in feats::CRITICAL_STRIKE {
        sheet.feats.0[usize::from(row / 64)] &= !(1u64 << (row % 64));
    }
    sheet
}

/// Clashes and locks: two medium attackers swinging top-down at each other,
/// 20 s from each of four seeds, at offence 3 and at offence 1; the tree's
/// outcomes, and the locks that came of them, one side pressing attack ten
/// times a second, the other not.
fn clashes(level: &Level, report: &mut Report, bouts: &mut Bouts) -> serde_json::Value {
    outln!("clashes: two medium sabers swinging at each other (offence 3, then 1)");
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut locks = Vec::new();
    let mut clashes = 0;
    for (offence, sheet) in [(3, swordsman(Style::Medium)), (1, plain_swordsman())] {
        for seed in 1..=4u64 {
            let mut bout = Bout::new(level, [guarded(sheet); 2], 52.0, seed);
            bout.light();
            let presser = (seed % 2) as u8;
            let end = bout.now() + 20_000;
            let mut locked_at: Option<(u64, i32, i32)> = None;
            while bout.now() < end {
                let in_lock = bout.fighter(0).saber.lock.is_some();
                bout.step(clashing(&bout, presser, in_lock));
                let events = bout.events();
                let (na, nb) = (bout.fighter(0), bout.fighter(1));
                if events.iter().any(|(_, e)| e.result == Outcome::Parried) && !in_lock {
                    clashes += 1;
                    for (seat, f) in [(0, na), (1, nb)] {
                        let mv = f.saber.current_move;
                        let kind = if f.saber.lock.is_some() {
                            "lock"
                        } else if mv.broken() {
                            "broken"
                        } else if mv.knockaway() {
                            "knockaway"
                        } else if mv.bounce() || (Move::D1_BR..=Move::D1_B_).contains(&mv) {
                            "bounce or deflection"
                        } else if f.saber.blocked != sim::combat::saber::Blocked::None {
                            "blocked, to react"
                        } else {
                            "on"
                        };
                        *outcomes.entry(format!("seat {seat}: {kind}")).or_insert(0) += 1;
                    }
                }
                if let (Some(la), Some(lb), None) = (na.saber.lock, nb.saber.lock, locked_at) {
                    locked_at = Some((bout.now(), la.frame, lb.frame));
                    outln!(
                        "  offence {offence}, seed {seed}: a lock at {} ms, {} against {}, frames {} / {}; seat {presser} presses",
                        bout.now(),
                        bout.ps(0).torso.anim.name(),
                        bout.ps(1).torso.anim.name(),
                        la.frame,
                        lb.frame
                    );
                }
                if let Some((from, fa, fb)) = locked_at
                    && in_lock
                    && na.saber.lock.is_none()
                {
                    let anims = [bout.ps(0).torso.anim, bout.ps(1).torso.anim];
                    let downs = [na.saber.floored(), nb.saber.floored()];
                    let winner = (0..2u8).find(|&seat| wins(anims[usize::from(seat)]));
                    let super_break = anims.iter().any(|a| a.name().contains("_SB_1_"));
                    let loser = 1 - presser;
                    outln!(
                        "    broke after {} ms: seat 0 plays {}, seat 1 {}; winner {winner:?}, super-break {super_break}, the loser knocked down {}",
                        bout.now() - from,
                        anims[0].name(),
                        anims[1].name(),
                        downs[usize::from(loser)]
                    );
                    locks.push(serde_json::json!({
                        "offence": offence, "seed": seed, "at": from, "frames": [fa, fb],
                        "presser": presser, "winner": winner, "super_break": super_break,
                        "loser_down": downs[usize::from(loser)], "lasted_ms": bout.now() - from,
                        "anims": [anims[0].name(), anims[1].name()],
                    }));
                    locked_at = None;
                }
            }
            bouts.push((format!("clashes-o{offence}-s{seed}"), bout.finish()));
        }
    }
    outln!("  {clashes} clashes: {outcomes:?}");
    report.check("blades clash", clashes > 0, format!("{clashes}"));
    report.check(
        "a clash's tree moves the blades (bounces, deflections, knockaways, breaks)",
        outcomes.keys().any(|k| !k.ends_with("on")),
        format!("{outcomes:?}"),
    );
    let presser_won = locks.iter().filter(|l| l["winner"] == l["presser"]).count();
    let knocked = locks.iter().filter(|l| l["loser_down"] == true).count();
    let supers = locks.iter().filter(|l| l["super_break"] == true).count();
    report.check(
        "locks come of clashes, and the side that presses wins each",
        !locks.is_empty() && presser_won == locks.len(),
        format!(
            "{} locks, {presser_won} won by the presser ({supers} super-breaks, {knocked} losers knocked down)",
            locks.len()
        ),
    );
    serde_json::json!({ "clashes": clashes, "outcomes": outcomes, "locks": locks })
}

/// Both seats running in swinging top-down; in a lock, `presser` pressing
/// attack ten times a second (48 ms down, 48 up) and the other nothing.
fn clashing(bout: &Bout, presser: u8, in_lock: bool) -> [Keys; 2] {
    if !in_lock {
        return [Keys::press(ATTACK).with(127, 0); 2];
    }
    let down = (bout.now() / 48).is_multiple_of(2);
    let mut keys = [Keys::default(); 2];
    keys[usize::from(presser)] = Keys::press(if down { ATTACK } else { 0 });
    keys
}

/// P12 (I30): a player who loses a lock at 1 HP is downed by JA's roll,
/// never killed: two plain medium swordsmen clashing (a lock's strength of
/// 2, so few super-breaks spare the loser), the presser's foe brought to 1
/// HP as their lock begins, sixteen seeds.
fn lock_down(level: &Level, report: &mut Report, bouts: &mut Bouts) -> serde_json::Value {
    outln!("lock_down: locks lost at 1 HP by a player (I30 P12)");
    let (mut locks, mut downed, mut killed) = (0, 0, 0);
    for seed in 1..=16u64 {
        let mut bout = Bout::new(level, [guarded(plain_swordsman()); 2], 52.0, seed);
        bout.light();
        let presser = (seed % 2) as u8;
        let loser = 1 - presser;
        let end = bout.now() + 20_000;
        let mut wounded = false;
        while bout.now() < end {
            let in_lock = bout.fighter(0).saber.lock.is_some();
            if in_lock && !wounded {
                bout.wound(loser, 100);
                wounded = true;
            }
            bout.step(clashing(&bout, presser, in_lock));
            if wounded && bout.fighter(0).saber.lock.is_none() {
                break;
            }
        }
        if wounded {
            locks += 1;
            bout.hold(200, [Keys::default(); 2]);
            let fallen = bout.fighter(loser);
            outln!(
                "  seed {seed}: seat {loser} lost the lock at 1 HP: {:?}, knocked down {}",
                fallen.life,
                fallen.saber.floored()
            );
            downed += usize::from(fallen.downed());
            killed += usize::from(fallen.dead());
        }
        bouts.push((format!("lock-down-s{seed}"), bout.finish()));
    }
    report.check(
        "a player who loses a lock at 1 HP is downed, never killed (I30 P12)",
        downed > 0 && killed == 0,
        format!("{locks} locks, {downed} downed, {killed} killed"),
    );
    serde_json::json!({ "locks": locks, "downed": downed, "killed": killed })
}

/// Throws: seat 0 throws (level 1, then Advanced, level 2) at seat 1 250
/// units away; then at a defender with defence 2, which knocks a level-1
/// throw down.
fn throws(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("throws: alternate held 1.5 s at a dummy 250 units away");
    let mut rows = Vec::new();
    for (row, name, defence) in [
        (49, "Throw Lightsaber", 0),
        (4, "Advanced", 0),
        (49, "Throw Lightsaber", 2),
    ] {
        let mut thrower = jedi(weapons_for(Style::Medium), 100);
        thrower.powers.insert(row);
        let mut target = with_defence(jedi(weapons_for(Style::Medium), 100), defence);
        target.max_hp = 2000;
        let mut bout = Bout::new(level, [thrower, target], 250.0, 5);
        bout.light();
        let fp_before = bout.fighter(0).fp;
        let (mut out_max, mut back_at, mut caught_at, mut down_at) = (0.0f32, None, None, None);
        let mut hits = 0;
        let mut fp_in_flight: Vec<(u64, i32)> = Vec::new();
        let start = bout.now();
        let mut recalled = false;
        let end = start + 12_000;
        while bout.now() < end {
            let t = bout.now() - start;
            let thrown = bout.fighter(0).saber.thrown;
            let lying = thrown.is_some_and(|t| t.flight == Flight::Lying);
            let keys0 = if t < 1500 {
                Keys::press(ALT)
            } else if lying && t > 1500 {
                Keys::press(ATTACK)
            } else {
                Keys::default()
            };
            bout.step([keys0, Keys::default()]);
            let f = bout.fighter(0);
            match f.saber.thrown {
                Some(thrown) => {
                    let from = bout.ps(0).origin;
                    out_max = out_max.max(sim::math::length(sim::math::sub(thrown.at, from)));
                    if thrown.flight == Flight::Back && back_at.is_none() {
                        back_at = Some(t);
                    }
                    if thrown.down() && down_at.is_none() {
                        down_at = Some(t);
                    }
                    if thrown.flight == Flight::Back && down_at.is_some() {
                        recalled = true;
                    }
                    if thrown.flying() {
                        fp_in_flight.push((bout.last.tick, f.fp));
                    }
                }
                None if caught_at.is_none() && back_at.is_some() => caught_at = Some(t),
                None => {}
            }
            for (seat, event) in bout.events() {
                if seat == 1 && matches!(event.result, Outcome::Hit | Outcome::Critical) {
                    hits += 1;
                }
            }
            if caught_at.is_some() {
                break;
            }
        }
        let paid = fp_before - fp_in_flight.first().map_or(fp_before, |(_, fp)| *fp);
        // No refill from one flying tick to the next.
        let steady = fp_in_flight
            .windows(2)
            .all(|w| w[1].0 != w[0].0 + 1 || w[1].1 <= w[0].1);
        outln!(
            "  {name} at defence {defence}: FP paid {:.0}, out to {out_max:.0} units, back at {back_at:?} ms, knocked down at {down_at:?}, recalled {recalled}, caught at {caught_at:?} ms, {hits} hits; the pool refilled in flight: {}",
            f64::from(paid) / 100.0,
            !steady
        );
        let blocked = defence > 1;
        report.check(
            format!(
                "{name} at defence {defence}: {}",
                if blocked {
                    "knocked down, called back, caught"
                } else {
                    "flies, hits, comes back, caught"
                }
            ),
            caught_at.is_some()
                && paid == 2000
                && steady
                && if blocked {
                    down_at.is_some() && recalled
                } else {
                    hits > 0
                },
            format!(
                "paid {}, hits {hits}, back {back_at:?}, down {down_at:?}, caught {caught_at:?}",
                paid / 100
            ),
        );
        rows.push(serde_json::json!({
            "throw": name, "defence": defence, "paid_fp": paid / 100, "out_max": out_max,
            "back_ms": back_at, "down_ms": down_at, "recalled": recalled, "caught_ms": caught_at,
            "hits": hits, "pool_paused": steady,
        }));
    }
    serde_json::json!(rows)
}

/// Kicks: the staff's forward kick at an unarmed dummy (tossed and
/// knocked down) and at an idle saber holder on the ground (tossed, not
/// knocked down).
fn kicks(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("kicks: the staff's forward kick at 40 units");
    let mut rows = Vec::new();
    for (name, weapons) in [
        ("an unarmed dummy", [None, None]),
        ("an idle saber holder", weapons_for(Style::Medium)),
    ] {
        let mut target = dummy(weapons);
        target.max_hp = 100;
        let mut bout = Bout::new(
            level,
            [jedi(weapons_for(Style::Staff), 100), target],
            40.0,
            6,
        );
        bout.light();
        let (mut hit, mut damage, mut lifted, mut down) = (0, 0, false, false);
        let end = bout.now() + 1500;
        while bout.now() < end {
            bout.step([Keys::press(ALT).with(127, 0), Keys::default()]);
            for (seat, event) in bout.events() {
                if seat == 1 && event.result == Outcome::Hit {
                    hit += 1;
                    damage += event.damage;
                }
            }
            lifted |= bout.ps(1).velocity[2] > 100.0;
            down |= bout.fighter(1).saber.floored();
        }
        let floors = weapons[0].is_none();
        outln!(
            "  {name}: {hit} hit ({:.2} HP of 100), lifted {lifted}, knocked down {down} (legs {})",
            f64::from(damage) / 100.0,
            bout.ps(0).legs.anim.name()
        );
        report.check(
            format!(
                "a kick at {name}: one hit, tossed{}",
                if floors {
                    ", knocked down"
                } else {
                    ", not knocked down"
                }
            ),
            // 2 or 3 JA points (of 100 HP) by where it landed (× 0.5 to 1.3).
            hit == 1 && lifted && down == floors && (100..=390).contains(&damage),
            format!("{hit} hit, {damage} hundredths, down {down}"),
        );
        rows.push(serde_json::json!({
            "target": name, "hits": hit, "damage": damage, "lifted": lifted, "down": down,
        }));
    }
    serde_json::json!(rows)
}

/// The roll stab (rulebook 2.8, 4.6): the fast style runs at a dummy, rolls
/// forward and stabs from the roll's end; the stab must cut.
fn roll_stab(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("roll stab: the fast style rolls at a dummy and stabs");
    let mut bout = Bout::new(
        level,
        [swordsman(Style::Fast), dummy([None, None])],
        ROLL_STAB_APART,
        7,
    );
    bout.light();
    bout.take_style(0, Style::Fast);
    let run = Keys::default().with(127, 0);
    let roll = Keys { up: -127, ..run };
    let (mut stabbed, mut hits, mut damage) = (false, 0, 0);
    let mut closest = f32::MAX;
    let end = bout.now() + 3000;
    let start = bout.now();
    while bout.now() < end {
        let stabbing = bout.fighter(0).saber.current_move == Move::ROLL_STAB;
        // Attack held from the roll until the stab is over, then let go so
        // no swing follows it.
        let keys = match bout.now() - start {
            ms if ms < 600 => run,
            _ if bout.ps(0).legs.anim != motion::Anim::BOTH_ROLL_F && !stabbed => roll,
            _ if stabbed && !stabbing => Keys::default(),
            _ => Keys::press(ATTACK),
        };
        bout.step([keys, Keys::default()]);
        let stabbing = bout.fighter(0).saber.current_move == Move::ROLL_STAB;
        stabbed |= stabbing;
        if stabbing {
            closest = closest.min(bout.ps(1).origin[0] - bout.ps(0).origin[0]);
        }
        for (seat, event) in bout.events() {
            if seat == 1 && event.result == Outcome::Hit && stabbing {
                hits += 1;
                damage += event.damage;
            }
        }
    }
    outln!(
        "  stabbed {stabbed}, {hits} hits ({:.2} HP), {closest:.0} units apart at the closest",
        f64::from(damage) / 100.0
    );
    report.check(
        "the roll stab cuts the dummy",
        stabbed && hits > 0,
        format!("stabbed {stabbed}, {hits} hits"),
    );
    serde_json::json!({ "stabbed": stabbed, "hits": hits, "damage": damage, "closest": closest })
}

/// How far the dummy stands from the roll stab's start: a 600 ms run and
/// the forward roll bring the stab within reach.
const ROLL_STAB_APART: f32 = 300.0;

/// Disarms: a strong saber and a fast one swinging at each other 20 s from
/// each of sixteen seeds (rulebook 8.4: the strong breaks the fast one's
/// guard and may knock its saber away, 70% × 50%, of the few clashes the
/// chance to miss lets come);
/// the saber lies 3 s, then its owner, still holding attack, calls it back.
fn disarms(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("disarms: strong against fast, both swinging, 20 s from sixteen seeds");
    let (mut lost, mut back) = ([0usize; 2], [0usize; 2]);
    for seed in 7..=22u64 {
        let mut bout = Bout::new(
            level,
            [
                guarded(swordsman(Style::Strong)),
                guarded(swordsman(Style::Fast)),
            ],
            52.0,
            seed,
        );
        bout.light();
        bout.take_style(0, Style::Strong);
        bout.take_style(1, Style::Fast);
        let mut was_down = [false; 2];
        let end = bout.now() + 20_000;
        while bout.now() < end {
            bout.step([Keys::press(ATTACK).with(127, 0); 2]);
            bout.events();
            for seat in 0..2u8 {
                let thrown = bout.fighter(seat).saber.thrown;
                let down = thrown.is_some_and(|t| t.down());
                let away = thrown.is_some();
                let s = usize::from(seat);
                if down && !was_down[s] {
                    lost[s] += 1;
                }
                if was_down[s] && !away {
                    back[s] += 1;
                }
                was_down[s] = down || (was_down[s] && away);
            }
        }
    }
    outln!(
        "  sabers knocked away: strong {}, fast {}; back in hand: strong {}, fast {}",
        lost[0],
        lost[1],
        back[0],
        back[1]
    );
    report.check(
        "the strong style disarms the fast one, never the other way, and the saber comes back",
        lost[1] > 0 && lost[0] == 0 && back[1] > 0,
        format!("lost {lost:?}, back {back:?}"),
    );
    serde_json::json!({ "lost": lost, "back": back })
}

/// A thrown saber struck by a blade (rulebook 10.6): seat 0 throws at
/// seat 1, 260 units away and spinning its staff (an extra-defence move:
/// any meeting knocks the saber down), sixteen times, each throw a little
/// later in the spin.
fn struck(level: &Level, report: &mut Report) -> serde_json::Value {
    outln!("thrown sabers met by a blade: sixteen throws into a staff's spin");
    let (mut struck, mut hit) = (0, 0);
    for k in 0..16u64 {
        let mut thrower = jedi(weapons_for(Style::Medium), 200);
        thrower.powers.insert(49);
        let mut target = swordsman(Style::Staff);
        target.max_hp = 2000;
        target.max_fp = 1000;
        let mut bout = Bout::new(level, [thrower, target], 260.0, 20 + k);
        bout.light();
        let start = bout.now();
        let throw_at = start + k * 37;
        let mut knocked = false;
        while bout.now() < start + 4000 {
            let keys0 = if bout.now() >= throw_at && bout.now() < throw_at + 1500 {
                Keys::press(ALT)
            } else {
                Keys::default()
            };
            // Crouched, forward and attack: the staff's spin.
            let spin = Keys {
                up: -127,
                ..Keys::press(ATTACK).with(127, 0)
            };
            bout.step([keys0, spin]);
            if bout.fighter(0).saber.thrown.is_some_and(|t| t.down()) && !knocked {
                knocked = true;
                struck += 1;
            }
            for (seat, event) in bout.events() {
                if seat == 1 && event.result == Outcome::Hit && event.by == Some(bout.seat(0).id) {
                    hit += 1;
                }
            }
        }
    }
    outln!("  struck out of the air {struck} of 16; the saber cut the defender {hit} times");
    report.check(
        "a spinning blade knocks a flying saber out of the air",
        struck > 0,
        format!("{struck} of 16"),
    );
    serde_json::json!({ "struck": struck, "of": 16, "cuts": hit })
}

/// end_m01aa for the lock-loss scene, its world seeded `seed`: the Dark
/// Jedi hostile, its Min1HP lifted (as its scene's script does) and 1000 HP
/// to live through a few cuts while it locks; and where it is.
fn dark_jedi_stage(
    discovery: &Discovery,
    seed: u64,
    report: &mut Report,
) -> Option<(Stage, usize)> {
    let hostile = ["end_darkjedi01".to_string()];
    let prepared = crate::scenes::installer(discovery, "k1")
        .and_then(|installer| installer.prepare_one("end_m01aa", None));
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            report.check("end_m01aa installs", false, error);
            return None;
        }
    };
    let level = crate::stage::with_factions(&prepared.plan.level, &hostile, &[]);
    let mut data = level.module()?.as_ref().clone();
    let index = data.objects().position(|o| {
        o.kind == ObjectKind::Creature && o.tag.eq_ignore_ascii_case("end_darkjedi01")
    })?;
    if let Some(sheet) = data.object_mut(index)?.sheet.as_mut() {
        sheet.flags &= !sim::combat::sheet::flags::MIN_ONE_HP;
        sheet.max_hp = 1000;
    }
    let level = with_data(&prepared.plan.level, &data, level.anims().clone());
    let config = sim::WorldConfig {
        seed,
        seats: 1,
        ..sim::WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, Level::arena()).ok()?;
    if let Err(error) = world.install(level, None) {
        report.check("end_m01aa installs", false, error.to_string());
        return None;
    }
    let stage = Stage::of(&mut world);
    let target = stage.creature("end_darkjedi01")?;
    Some((stage, target))
}

/// C22's lock-loss kill: the dev Jedi against end_m01aa's Dark Jedi
/// (Min1HP lifted, 1000 HP, 100 left: cuts hardly move its health),
/// pressing attack five times a second (so it seldom has the hits left
/// for a super-break);
/// a lock it wins with hits in hand knocks the creature down, and JA's roll
/// (0 to 40 above its health) may kill it.
fn lock_kill(discovery: &Discovery, report: &mut Report) -> serde_json::Value {
    outln!("lock-loss kill (C22): the dev Jedi against end_m01aa's Dark Jedi at 100 of 1000 HP");
    let (mut locks, mut won, mut kills, mut downs) = (0, 0, 0, 0);
    // Each attempt in a world of another seed: the server's rolls differ.
    for attempt in 0..40u64 {
        let Some((stage, target)) = dark_jedi_stage(discovery, attempt + 1, report) else {
            break;
        };
        let apart = 56.0;
        let Some(mut start) = stage.scene(target, apart, guarded(Sheet::dev_jedi())) else {
            break;
        };
        if let Some(fighter) = start.entities[target].fighter.as_mut() {
            fighter.hp = 100 * 100;
        }
        let me = scenes::seat0(&start);
        // Five presses a second, a key held so it is never idle (JA locks
        // no idle player).
        let run = stage.run(&start, target, 30_000, &mut |ms| {
            (
                if (ms / 100).is_multiple_of(2) {
                    ATTACK
                } else {
                    0
                },
                0,
                1,
            )
        });
        let mut locked = false;
        let clashes = run
            .snapshots
            .iter()
            .filter(|s| {
                scenes::fighter(s, me)
                    .newest()
                    .next()
                    .is_some_and(|e| e.tick == s.tick && e.result == Outcome::Parried)
            })
            .count();
        let died_at = run
            .snapshots
            .iter()
            .find(|s| !scenes::fighter(s, target).alive())
            .map(|s| s.time_ms - start.time_ms);
        outln!("  attempt {attempt}: {clashes} clashes, the Dark Jedi died at {died_at:?} ms");
        for snapshot in &run.snapshots {
            let mine = scenes::fighter(snapshot, me);
            let theirs = scenes::fighter(snapshot, target);
            if mine.saber.lock.is_some() && !locked {
                locked = true;
                locks += 1;
            }
            if locked && mine.saber.lock.is_none() {
                locked = false;
                let after = run
                    .snapshots
                    .iter()
                    .filter(|s| s.tick >= snapshot.tick && s.tick < snapshot.tick + 20)
                    .map(|s| scenes::fighter(s, target));
                let (mut died, mut down) = (false, false);
                for f in after {
                    died |= !f.alive();
                    down |= f.saber.floored();
                }
                let jedi = scenes::player(snapshot, me).torso.anim;
                won += usize::from(died || down || wins(jedi));
                kills += usize::from(died);
                downs += usize::from(down);
                outln!(
                    "  attempt {attempt}: a lock ended at {} ms: the Jedi plays {}, the Dark Jedi {}; knocked down {down}, killed {died}",
                    snapshot.time_ms - start.time_ms,
                    scenes::player(snapshot, me).torso.anim.name(),
                    scenes::player(snapshot, target).torso.anim.name()
                );
            }
            if !theirs.alive() {
                break;
            }
        }
        if kills > 0 {
            break;
        }
    }
    outln!("  {locks} locks, {won} won, {downs} knockdowns, {kills} kills");
    report.check(
        "a creature that loses a lock with its winner's hits in hand may die (C22)",
        kills > 0,
        format!("{locks} locks, {won} won, {downs} knocked down, {kills} killed"),
    );
    serde_json::json!({ "locks": locks, "won": won, "knockdowns": downs, "kills": kills })
}

/// end_m01aa planned for one seat, timed by `anims`: its level (the
/// cost scenes' start).
pub(crate) fn end_m01aa(
    discovery: &Discovery,
    anims: &Arc<AnimTable>,
    report: &mut Report,
) -> Option<Level> {
    let prepared = crate::lifecycle::Installer::new(discovery, vfs::Game::Kotor1, anims.clone())
        .and_then(|installer| installer.prepare_one("end_m01aa", None));
    match prepared {
        Ok(prepared) => Some(prepared.plan.level),
        Err(error) => {
            report.check("end_m01aa installs", false, error);
            None
        }
    }
}

/// `plan`'s module with the `users` creatures nearest the Dark Jedi given
/// sabers and set on each other (every other one on the player's side),
/// the rest standing aside; and where the Dark Jedi stands.
pub(crate) fn sword_users(plan: &Level, users: usize) -> (sim::module::ModuleData, [f32; 3]) {
    let mut data = plan.module().expect("a module").as_ref().clone();
    let anchor = data
        .objects()
        .find(|o| o.tag.eq_ignore_ascii_case("end_darkjedi01"))
        .map_or([0.0; 3], |o| o.position);
    let mut nearest: Vec<(usize, f32)> = data
        .objects()
        .enumerate()
        .filter(|(_, o)| o.kind == ObjectKind::Creature && o.sheet.is_some())
        .map(|(i, o)| (i, sim::math::length(sim::math::sub(o.position, anchor))))
        .collect();
    nearest.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let chosen: BTreeSet<usize> = nearest.iter().take(users).map(|&(i, _)| i).collect();
    let mut given = 0;
    data.change_each(
        |o| o.sheet.is_some(),
        |index, object| {
            let Some(sheet) = object.sheet.as_mut() else {
                return;
            };
            if chosen.contains(&index) {
                sheet.weapons = [Some(saber(8)), None];
                sheet.feats.insert(feats::PROFICIENCY_LIGHTSABER);
                sheet.faction = if given % 2 == 0 {
                    factions::HOSTILE_1
                } else {
                    factions::PLAYER
                };
                sheet.max_hp = sheet.max_hp.max(200);
                given += 1;
            } else {
                sheet.faction = factions::NEUTRAL;
            }
        },
    );
    (data, anchor)
}

/// `plan`'s level with the module `data`, its animations `table`'s.
pub(crate) fn with_data(
    plan: &Level,
    data: &sim::module::ModuleData,
    table: Arc<AnimTable>,
) -> Level {
    let level = Level::new(plan.name(), plan.collision().clone(), table).with_module(data.clone());
    match plan.nav() {
        Some(nav) => level.with_nav(nav.as_ref().clone()),
        None => level,
    }
}

/// The time (µs) of one step in a world restored from `from` in each
/// level, stepped first through `warm` (untimed: its history fills), then
/// once more with each input: the same state's step, timed each way (the
/// snapshot's module renamed to each level's).
pub(crate) fn timed(
    from: &Snapshot,
    rate: TickRate,
    warm: &[TickInput],
    variants: &[(&Level, &TickInput)],
) -> Vec<f64> {
    variants
        .iter()
        .map(|(level, input)| {
            let mut start = from.clone();
            start.place = level.place();
            let mut copy = SimWorld::from_snapshot(&start, (*level).clone()).expect("restores");
            let mut tick = from.tick;
            for input in warm {
                tick += 1;
                step(
                    &mut copy,
                    tick,
                    input,
                    rate.msec(tick),
                    StepReason::Authority,
                );
            }
            tick += 1;
            let started = Instant::now();
            step(
                &mut copy,
                tick,
                input,
                rate.msec(tick),
                StepReason::Authority,
            );
            started.elapsed().as_secs_f64() * 1e6
        })
        .collect()
}

/// A list's mean.
pub(crate) fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

/// A list's median (its upper middle).
pub(crate) fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// The cost: end_m01aa with `users` creatures given sabers and set on each
/// other (every other one hostile), stepped 15 s with the skeleton and
/// again without it (no blades placed): a tick's mean time either way.
fn cost(discovery: &Discovery, anims: &Arc<AnimTable>, report: &mut Report) -> serde_json::Value {
    outln!("cost: end_m01aa, sword users fighting, 15 s stepped with and without the blades");
    let Some(plan) = end_m01aa(discovery, anims, report) else {
        return serde_json::Value::Null;
    };
    let bare = Arc::new(anims.as_ref().clone().without_skeleton());
    let mut rows = Vec::new();
    for users in [2usize, 8, 26] {
        let (data, _) = sword_users(&plan, users);
        let (with_level, bare_level) = (
            with_data(&plan, &data, anims.clone()),
            with_data(&plan, &data, bare.clone()),
        );
        let config = sim::WorldConfig {
            seed: 1,
            seats: 1,
            ..sim::WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, Level::arena()).expect("one seat");
        if let Err(error) = world.install(with_level.clone(), None) {
            report.check("end_m01aa installs", false, error.to_string());
            return serde_json::Value::Null;
        }
        // Each sampled tick is stepped twice from the same snapshot, into a
        // world with the blades and one without: the difference is the
        // blades' work on the very same state.
        let rate = TickRate::default();
        let mut last = world.snapshot();
        let (mut with, mut without) = (Vec::new(), Vec::new());
        let (mut placed, mut swinging, mut samples) = (0u64, 0u64, 0u64);
        let input = TickInput::default();
        while rate.time_ms(last.tick) < 15_000 {
            let tick = last.tick + 1;
            if tick.is_multiple_of(4) {
                let times = timed(
                    &last,
                    rate,
                    &[],
                    &[(&with_level, &input), (&bare_level, &input)],
                );
                with.push(times[0]);
                without.push(times[1]);
            }
            last = step(
                &mut world,
                tick,
                &input,
                rate.msec(tick),
                StepReason::Authority,
            );
            samples += 1;
            for entity in &last.entities {
                if let Some(f) = entity.fighter {
                    placed += f
                        .trails
                        .iter()
                        .flatten()
                        .filter(|t| t.at_ms == last.time_ms)
                        .count() as u64;
                    swinging += u64::from(f.saber.current_move.attack());
                }
            }
        }
        let differences: Vec<f64> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
        let (mean_with, mean_without, blades) = (mean(&with), mean(&without), mean(&differences));
        let (median_with, median_without) = (median(&with), median(&without));
        let placed = placed as f64 / samples as f64;
        let swinging = swinging as f64 / samples as f64;
        outln!(
            "  {users:>2} sword users: a tick {mean_with:>6.1} µs with the blades, {mean_without:>6.1} without (medians {median_with:.1}, {median_without:.1}): the blades {blades:+.1} µs; {placed:.1} blades placed, {swinging:.1} swinging a tick"
        );
        rows.push(serde_json::json!({
            "users": users, "with_us": mean_with, "without_us": mean_without,
            "median_with_us": median_with, "median_without_us": median_without,
            "blades_us": blades, "placed_per_tick": placed, "swinging_per_tick": swinging,
            "samples": with.len(),
        }));
    }
    report.check(
        "the cost was measured",
        rows.len() == 3,
        format!("{} runs", rows.len()),
    );
    serde_json::json!(rows)
}

/// Each bout as `<folder>/<name>.kjkr` (`kjk play --replay` draws it).
pub(crate) fn write_bouts(folder: &std::path::Path, bouts: &Bouts) {
    for (name, recording) in bouts {
        let path = folder.join(format!("{name}.kjkr"));
        if let Err(error) = std::fs::write(&path, recording.encode()) {
            eprintln!("{}: {error}", path.display());
        }
    }
    if !bouts.is_empty() {
        eprintln!("bouts: {} in {}", bouts.len(), folder.display());
    }
}

/// `kjk combat duel`.
pub fn duel(discovery: &Discovery, only: &[String]) -> ExitCode {
    let (anims, note) = crate::stage::anims(discovery);
    outln!("combat duel: {note}");
    if anims.skeleton().is_none() {
        eprintln!("the duels need JA's skeleton and sabers");
        return ExitCode::FAILURE;
    }
    let anims = Arc::new(anims);
    let level = ring(&anims);
    let mut report = Report::default();
    let mut bouts = Bouts::new();
    type Section<'a> = Box<dyn Fn(&mut Report, &mut Bouts) -> serde_json::Value + 'a>;
    let sections: [(&str, Section); 14] = [
        ("cuts", Box::new(|r, _| cuts(&level, r))),
        ("feat_attacks", Box::new(|r, _| feat_attacks(r))),
        ("blocks", Box::new(|r, b| blocks(&level, r, b))),
        ("bolts", Box::new(|r, _| bolts(&level, r))),
        ("blade_bolts", Box::new(|r, b| blade_bolts(&level, r, b))),
        ("clashes", Box::new(|r, b| clashes(&level, r, b))),
        ("lock_down", Box::new(|r, b| lock_down(&level, r, b))),
        ("disarms", Box::new(|r, _| disarms(&level, r))),
        ("lock_kill", Box::new(|r, _| lock_kill(discovery, r))),
        ("throws", Box::new(|r, _| throws(&level, r))),
        ("struck", Box::new(|r, _| struck(&level, r))),
        ("kicks", Box::new(|r, _| kicks(&level, r))),
        ("roll_stab", Box::new(|r, _| roll_stab(&level, r))),
        ("cost", Box::new(|r, _| cost(discovery, &anims, r))),
    ];
    if let Some(unknown) = only
        .iter()
        .find(|o| !sections.iter().any(|(name, _)| name == o))
    {
        let names: Vec<&str> = sections.iter().map(|(name, _)| *name).collect();
        eprintln!("--only {unknown}: one of {}", names.join(", "));
        return ExitCode::from(2);
    }
    let mut facts = serde_json::json!({ "verb": "combat duel" });
    for (name, run) in &sections {
        if only.is_empty() || only.iter().any(|o| o == name) {
            facts[*name] = run(&mut report, &mut bouts);
        }
    }
    let failed = report.checks.iter().filter(|c| !c.pass).count();
    outln!(
        "combat duel: {} checks, {failed} failed",
        report.checks.len()
    );
    for check in report.checks.iter().filter(|c| !c.pass) {
        outln!("  FAIL {}: {}", check.name, check.seen);
    }
    facts["checks"] = serde_json::json!(
        report
            .checks
            .iter()
            .map(|c| serde_json::json!({"name": c.name, "pass": c.pass, "seen": c.seen}))
            .collect::<Vec<_>>()
    );
    if let Some(folder) = crate::runtime::write_facts(facts) {
        write_bouts(&folder, &bouts);
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
