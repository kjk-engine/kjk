//! `kjk combat saber`: the saber moves (`docs/COMBAT.md`) through
//! move_body's weapon step, without a window: a character per style on
//! the arena's floor, KOTOR's feats
//! giving it the style, driven by scripted commands as a player's keys
//! would. Every move is printed as it starts (its name in the table, the
//! torso's and legs' animations, its timer, the Force it cost); the
//! rulebook's rules are checked (chain limits, costs, timing against the
//! rig's timelines, knockdowns, weapon changes, style cycling) and the
//! results go to `facts.json`.

use std::process::ExitCode;

use collision::{Bodies, Body, CollisionWorld, Contents, UNIT};
use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::Gla;
use sim::combat::Fighter;
use sim::combat::saber::{Getup, Hold, Kit, Move, Style};
use sim::combat::sheet::{Sheet, Weapon, damage_types, feats};
use sim::motion::{self, Anim, AnimTable, Arms, Other, MotionState, events};
use sim::{BODY, Command, SimRng, buttons};
use vfs::Discovery;

use crate::rig_timeline;

/// Every step is one of JA's 8 ms commands.
const MSEC: u32 = 8;
pub(crate) const ATTACK: u16 = buttons::ATTACK;
pub(crate) const ALT: u16 = buttons::ALT_ATTACK;

/// The keys a step presses.
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    forward: i8,
    right: i8,
    up: i8,
    buttons: u16,
    weapon: u8,
}

impl Keys {
    fn press(buttons: u16) -> Keys {
        Keys {
            buttons,
            ..Keys::default()
        }
    }

    fn with(self, forward: i8, right: i8) -> Keys {
        Keys {
            forward,
            right,
            ..self
        }
    }

    fn up(self, up: i8) -> Keys {
        Keys { up, ..self }
    }
}

/// A move as it started.
#[derive(Debug, Clone, Copy)]
struct Start {
    at: u64,
    mv: Move,
    style: Option<Style>,
    torso: Anim,
    legs: Anim,
    timer: i32,
    /// Attacks since the last ready (JA's chain counter).
    chain: u8,
    fp_before: i32,
    fp_after: i32,
}

/// Another character standing by: a box and what its legs play.
struct Dummy {
    body: Body,
    other: Other,
}

/// One character on the floor and what its moves did.
struct Duel<'a> {
    world: &'a dyn CollisionWorld,
    anims: &'a AnimTable,
    kit: Kit,
    ps: MotionState,
    fighter: Fighter,
    rng: SimRng,
    now: u64,
    dummies: Vec<Dummy>,
    starts: Vec<Start>,
    /// The step's events, all of the run (move_body's `events` bits).
    events: u8,
}

impl<'a> Duel<'a> {
    fn new(sheet: &Sheet, world: &'a dyn CollisionWorld, anims: &'a AnimTable) -> Duel<'a> {
        // On the arena's floor, looking along +Y where nothing stands.
        let mut ps = MotionState::standing([0.0, 0.0, 0.125 - motion::SOLE_Z], 90.0);
        ps.ground = None;
        let mut duel = Duel {
            world,
            anims,
            kit: Kit::of(sheet),
            ps,
            fighter: Fighter::of(sheet),
            rng: SimRng::new(24, 3),
            now: 0,
            dummies: Vec::new(),
            starts: Vec::new(),
            events: 0,
        };
        duel.idle(400);
        duel
    }

    fn step(&mut self, keys: Keys) {
        let command = Command {
            angles: self.ps.command_angles,
            forward: keys.forward,
            right: keys.right,
            up: keys.up,
            buttons: keys.buttons,
            weapon: keys.weapon,
            ..Command::default()
        };
        let bodies: Vec<Body> = self.dummies.iter().map(|dummy| dummy.body).collect();
        let others: Vec<Other> = self.dummies.iter().map(|dummy| dummy.other).collect();
        let before = (
            self.fighter.saber.current_move,
            self.ps.torso.flip,
            self.ps.legs.anim,
        );
        let fp_before = self.fighter.fp;
        self.now += u64::from(MSEC);
        let world = Bodies::new(self.world, &bodies);
        let mut arms = Arms {
            fighter: &mut self.fighter,
            kit: &self.kit,
            player: true,
            others: &others,
            rng: &mut self.rng,
            enemy: None,
        };
        motion::move_body(
            &mut self.ps,
            &command,
            MSEC,
            self.now,
            &world,
            self.anims,
            Some(&mut arms),
        );
        self.events |= self.ps.events;
        let saber = self.fighter.saber;
        let restarted = saber.current_move == before.0
            && self.ps.torso.flip != before.1
            && !saber.current_move.idle();
        // The aerial cartwheels play on the legs alone and set no move.
        let cartwheel = self.ps.legs.anim != before.2
            && matches!(
                self.ps.legs.anim,
                Anim::BOTH_ARIAL_LEFT | Anim::BOTH_ARIAL_RIGHT
            );
        if saber.current_move != before.0 || restarted || cartwheel {
            self.starts.push(Start {
                at: self.now,
                mv: saber.current_move,
                style: saber.style,
                torso: self.ps.torso.anim,
                legs: self.ps.legs.anim,
                timer: if cartwheel {
                    self.ps.legs.timer
                } else {
                    self.ps.torso.timer
                },
                chain: saber.chain,
                fp_before,
                fp_after: self.fighter.fp,
            });
        }
    }

    fn hold(&mut self, ms: u64, keys: Keys) {
        for _ in 0..ms / u64::from(MSEC) {
            self.step(keys);
        }
    }

    fn idle(&mut self, ms: u64) {
        self.hold(ms, Keys::default());
    }

    /// Steps `keys` until `done` (at most `ms`); whether it came.
    fn until(&mut self, ms: u64, keys: Keys, done: impl Fn(&Duel) -> bool) -> bool {
        for _ in 0..ms / u64::from(MSEC) {
            if done(self) {
                return true;
            }
            self.step(keys);
        }
        done(self)
    }

    /// Ready again: the move idle, its timer out, standing on the floor.
    fn settle(&mut self) -> bool {
        self.until(8000, Keys::default(), |duel| {
            matches!(duel.fighter.saber.current_move, Move::READY | Move::NONE)
                && duel.fighter.weapon_wait_ms <= 0
                && duel.ps.ground.is_some()
                && duel.ps.legs.timer <= 0
                && duel.ps.torso.timer <= 0
        })
    }

    /// Lights the saber (the first press of attack) and settles.
    fn light(&mut self) {
        if !self.fighter.saber.on {
            self.step(Keys::press(ATTACK));
        }
        self.settle();
        // Once more, for the lit stance.
        self.step(Keys::default());
    }

    /// Presses the style button until the saber fights in `style`.
    fn take_style(&mut self, style: Style) -> bool {
        for _ in 0..4 {
            if self.fighter.saber.style == Some(style) {
                return true;
            }
            self.step(Keys::press(buttons::SABER_STYLE));
            self.step(Keys::default());
        }
        self.fighter.saber.style == Some(style)
    }

    /// A character `ahead` and `right` of this one (units, along its view),
    /// its legs playing `legs` (lying low when knocked down).
    fn stand_by(&mut self, ahead: f32, right: f32, legs: Anim) {
        let (forward, side, _) = sim::math::angle_vectors([0.0, self.ps.view[1], 0.0]);
        let o = self.ps.origin;
        let at = [
            o[0] + forward[0] * ahead + side[0] * right,
            o[1] + forward[1] * ahead + side[1] * right,
            o[2],
        ];
        let top = if legs == Anim::BOTH_KNOCKDOWN1 {
            motion::CROUCH_HEIGHT
        } else {
            motion::STAND_HEIGHT
        };
        let entity = (self.dummies.len() as u32 + 1) | BODY;
        let half = motion::HALF_WIDTH;
        let metres = |v: [f32; 3]| v.map(|c| c * UNIT);
        self.dummies.push(Dummy {
            body: Body {
                entity,
                min: metres([at[0] - half, at[1] - half, at[2] + motion::SOLE_Z]),
                max: metres([at[0] + half, at[1] + half, at[2] + top]),
                contents: Contents::BODY,
            },
            other: Other { entity, legs },
        });
    }

    fn clear(&mut self) {
        self.dummies.clear();
    }

    /// The starts since `from`.
    fn since(&self, from: usize) -> &[Start] {
        &self.starts[from..]
    }
}

/// A rule checked, and what was seen.
pub(crate) struct Check {
    pub name: String,
    pub pass: bool,
    pub seen: String,
}

#[derive(Default)]
pub(crate) struct Report {
    pub checks: Vec<Check>,
    pub sections: Vec<serde_json::Value>,
}

impl Report {
    pub(crate) fn check(&mut self, name: impl Into<String>, pass: bool, seen: impl Into<String>) {
        let (name, seen) = (name.into(), seen.into());
        outln!(
            "  check {:<4} {name}: {seen}",
            if pass { "ok" } else { "FAIL" }
        );
        self.checks.push(Check { name, pass, seen });
    }

    /// Prints the moves `starts` and keeps them for the facts.
    fn section(&mut self, title: &str, starts: &[Start]) {
        outln!("  {title}");
        for start in starts {
            outln!(
                "    {:>6} ms  {:<22} {:<6} torso {:<24} legs {:<24} timer {:>5} chain {:>2}  FP {:>6.2} -> {:>6.2}",
                start.at,
                start.mv.name(),
                start.style.map_or("-", Style::name),
                start.torso.name(),
                start.legs.name(),
                start.timer,
                start.chain,
                f64::from(start.fp_before) / 100.0,
                f64::from(start.fp_after) / 100.0,
            );
        }
        self.sections.push(serde_json::json!({
            "title": title,
            "moves": starts.iter().map(|start| serde_json::json!({
                "at": start.at, "move": start.mv.name(),
                "style": start.style.map(Style::name),
                "torso": start.torso.name(), "legs": start.legs.name(),
                "timer": start.timer, "chain": start.chain,
                "fp_before": start.fp_before, "fp_after": start.fp_after,
            })).collect::<Vec<_>>(),
        }));
    }
}

/// A lightsaber of base item `row` (8 single, 9 double-bladed).
pub(crate) fn saber(row: u16) -> Weapon {
    Weapon::plain(
        row,
        2,
        8,
        (2, 2),
        damage_types::ENERGY,
        [
            feats::PROFICIENCY_LIGHTSABER,
            feats::WEAPON_FOCUS_LIGHTSABER,
            feats::WEAPON_SPEC_LIGHTSABER,
        ],
    )
}

pub(crate) fn pistol() -> Weapon {
    Weapon::plain(
        12,
        1,
        6,
        (1, 2),
        damage_types::ENERGY,
        [
            feats::PROFICIENCY_BLASTER,
            feats::WEAPON_FOCUS_BLASTER,
            feats::WEAPON_SPEC_BLASTER,
        ],
    )
}

/// A Jedi with `weapons`, every saber feat line to its top (so each style
/// is open, C8), Force Jump to its second tier and 100 FP: JA's costs come
/// out in JA's numbers.
pub(crate) fn jedi(weapons: [Option<Weapon>; 2], max_fp: u16) -> Sheet {
    let mut sheet = Sheet::commoner();
    let rows = [
        &[feats::PROFICIENCY_LIGHTSABER, feats::PROFICIENCY_BLASTER][..],
        &feats::CRITICAL_STRIKE,
        &feats::FLURRY,
        &feats::POWER_ATTACK,
        &feats::TWO_WEAPON_FIGHTING,
        &feats::JEDI_DEFENSE,
        &feats::FORCE_JUMP[..2],
    ];
    for row in rows.iter().flat_map(|rows| rows.iter()) {
        sheet.feats.insert(*row);
    }
    sheet.max_hp = 60;
    sheet.max_fp = max_fp;
    sheet.weapons = weapons;
    sheet
}

/// The single saber's styles, the pair's and the staff's.
pub(crate) const STYLES: [Style; 5] = [
    Style::Fast,
    Style::Medium,
    Style::Strong,
    Style::Dual,
    Style::Staff,
];

pub(crate) fn weapons_for(style: Style) -> [Option<Weapon>; 2] {
    match style {
        Style::Dual => [Some(saber(8)), Some(saber(8))],
        Style::Staff => [Some(saber(9)), None],
        _ => [Some(saber(8)), None],
    }
}

/// The rig's level for a style's timing (`asset_ja::saber_anim_speed`).
fn rig_level(style: Style) -> u32 {
    match style {
        Style::Fast => 1,
        Style::Strong => 3,
        _ => 2,
    }
}

/// A character of `style` with a lit saber, ready.
fn duelist<'a>(style: Style, world: &'a dyn CollisionWorld, anims: &'a AnimTable) -> Duel<'a> {
    let sheet = jedi(weapons_for(style), 100);
    let mut duel = Duel::new(&sheet, world, anims);
    duel.light();
    duel.take_style(style);
    duel.settle();
    duel
}

/// The movement keys a hold of attack is tried with.
const DIRECTIONS: [(&str, i8, i8); 9] = [
    ("none", 0, 0),
    ("forward", 127, 0),
    ("forward right", 127, 127),
    ("right", 0, 127),
    ("back right", -127, 127),
    ("back", -127, 0),
    ("back left", -127, -127),
    ("left", 0, -127),
    ("forward left", 127, -127),
];

/// How long attack is held with each direction.
const HOLD_MS: u64 = 6000;

/// JA's chain counter (attacks since the last ready) each time a return
/// broke the chain while attack was still held before `released`, and its
/// highest. A return does not reset the counter: held on, the chain may
/// pick up again from it (and end sooner).
fn chains(starts: &[Start], released: u64) -> (Vec<u8>, u8) {
    let limited = starts
        .iter()
        .filter(|start| start.mv.ret() && start.at <= released)
        .map(|start| start.chain)
        .collect();
    let most = starts.iter().map(|start| start.chain).max().unwrap_or(0);
    (limited, most)
}

/// Holds attack with each movement direction for `style`: prints the moves
/// and checks the chain limit (rulebook 2.6).
fn holds(style: Style, world: &dyn CollisionWorld, anims: &AnimTable, report: &mut Report) {
    for (name, forward, right) in DIRECTIONS {
        let mut duel = duelist(style, world, anims);
        let from = duel.starts.len();
        duel.hold(HOLD_MS, Keys::press(ATTACK).with(forward, right));
        let released = duel.now;
        duel.settle();
        let starts = duel.since(from).to_vec();
        report.section(
            &format!("{}: attack held {HOLD_MS} ms with {name}", style.name()),
            &starts,
        );
        let (limited, most) = chains(&starts, released);
        let seen = format!("chain counter {limited:?} at each break, {most} at most");
        let label = format!("{} {name}: chain limit", style.name());
        let broken = !limited.is_empty();
        let pass = match style {
            // With no key an attack's chain attack is its return: one swing
            // at a time.
            _ if forward == 0 && right == 0 => broken && most == 1,
            // Two attacks: a repeat's chain angle is 180°; held on out of
            // the return, a swing whose angle from the right is 135° to
            // 215° chains once more, past a roll of 2 never.
            Style::Strong => broken && limited.iter().all(|&n| (2..=3).contains(&n)) && most <= 3,
            // Done past a roll of 2 to 5: never past six.
            Style::Medium => broken && limited.iter().all(|&n| (3..=6).contains(&n)) && most <= 6,
            // Top to bottom (forward, or back with nobody behind) chains
            // without end; the spinning swings only past a tolerance of 5
            // and a roll of 1 to the chain over it: 6 at least.
            Style::Fast if right == 0 => !broken,
            Style::Fast => limited.iter().all(|&n| n >= 6),
            Style::Dual | Style::Staff => !broken,
        };
        report.check(label, pass, seen);
    }
}

/// The rig's combo for `style` and the keys that play it in the sim, each
/// held until the move it leads to starts (the last one to ready).
fn combo(style: Style) -> (Vec<String>, Vec<(Keys, Move)>) {
    let attack = Keys::press(ATTACK);
    let (names, steps): (Vec<String>, Vec<(Keys, Move)>) = match style {
        Style::Fast => (
            rig_timeline::FAST_COMBO
                .iter()
                .map(|s| s.to_string())
                .collect(),
            vec![
                (attack.with(127, 127), Move::A_TL2BR),
                (attack.with(127, -127), Move::A_TR2BL),
                (attack.with(127, 0), Move::A_T2B),
            ],
        ),
        Style::Medium => (
            rig_timeline::MEDIUM_COMBO
                .iter()
                .map(|s| s.to_string())
                .collect(),
            vec![
                (attack.with(127, 127), Move::A_TL2BR),
                (attack.with(127, -127), Move::A_TR2BL),
            ],
        ),
        Style::Strong => (
            rig_timeline::STRONG_COMBO
                .iter()
                .map(|s| s.to_string())
                .collect(),
            vec![(attack.with(127, 127), Move::R_TL2BR)],
        ),
        Style::Dual | Style::Staff => (
            rig_timeline::two_bladed_combo(style as u32).to_vec(),
            vec![
                (attack.with(127, 127), Move::A_TL2BR),
                (attack.with(127, -127), Move::A_TR2BL),
            ],
        ),
    };
    (names, steps)
}

/// The rig's hold for `name` at `style` and whether the sequence loops, if
/// the skeleton is at hand.
fn rig_hold(rig: Option<&(Gla, AnimationConfig)>, style: Style, name: &str) -> Option<(i32, bool)> {
    let (gla, cfg) = rig?;
    let hold = rig_timeline::saber_move_hold(gla, cfg, rig_level(style), name).ok()?;
    let looping = rig_timeline::find(cfg, name).ok()?.loop_frame != -1;
    Some((hold as i32, looping))
}

/// Compares each started move's timer with JA's rule (an active move holds
/// its frames but one, stretched by its style's speed, less 1 ms; a
/// finishing one all its frames), with the rig's timelines, and each
/// move's length in the sim with its timer. The rig holds every animation
/// one frame short, or all its frames when it loops, whatever the move:
/// JA's rule differs from it by a frame for returns and looping
/// animations, which the check expects.
fn timing(
    label: &str,
    style: Style,
    starts: &[Start],
    anims: &AnimTable,
    rig: Option<&(Gla, AnimationConfig)>,
    report: &mut Report,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for (at, start) in starts.iter().enumerate() {
        let data = start.mv.data();
        if data.hold == Hold::Idle {
            continue;
        }
        let info = anims.get(start.torso);
        let lerp = info.frame_lerp.abs();
        let rule = match data.hold {
            Hold::Active => info.move_hold_ms(match style {
                Style::Fast if is_transition(start.torso, 0) => 1.5,
                Style::Strong if is_transition(start.torso, 2) => 0.75,
                _ => 1.0,
            }),
            _ => info.length_ms(),
        };
        let rig_hold = rig_hold(rig, style, start.torso.name());
        let next = starts.get(at + 1).map(|next| next.at - start.at);
        outln!(
            "    {:<18} {:<24} timer {:>5} (rule {:>5}){}{}",
            start.mv.name(),
            start.torso.name(),
            start.timer,
            rule,
            rig_hold.map_or(String::new(), |(hold, looping)| format!(
                ", rig holds {hold}{}",
                if looping { " (looping)" } else { "" }
            )),
            next.map_or(String::new(), |ms| format!(", next move after {ms} ms")),
        );
        report.check(
            format!("{label} {}: timer is JA's", start.mv.name()),
            start.timer == rule,
            format!("{} ms, rule {rule} ms", start.timer),
        );
        if let Some((hold, looping)) = rig_hold {
            let (expected, what) = match (data.hold, looping) {
                (Hold::Active, false) => (hold - 1, "the rig's hold less 1 ms"),
                (Hold::Active, true) => (hold - lerp - 1, "a frame and 1 ms under the rig's"),
                (_, false) => (hold + lerp, "a frame over the rig's hold"),
                (_, true) => (hold, "the rig's hold"),
            };
            report.check(
                format!("{label} {}: agrees with the rig", start.mv.name()),
                start.timer == expected,
                format!("{} ms, {what} ({expected} ms)", start.timer),
            );
        }
        if let Some(next) = next {
            let late = next as i32 - start.timer;
            report.check(
                format!("{label} {}: lasts its timer", start.mv.name()),
                (0..MSEC as i32).contains(&late),
                format!("the next move {late} ms after the timer ran out (a step is {MSEC} ms)"),
            );
        }
        rows.push(serde_json::json!({
            "move": start.mv.name(), "anim": start.torso.name(), "timer": start.timer,
            "rule": rule, "rig_hold": rig_hold.map(|(hold, _)| hold),
            "rig_looping": rig_hold.map(|(_, looping)| looping), "next_after": next,
        }));
    }
    rows
}

/// Whether `anim` is among the style group `group`'s transitions.
fn is_transition(anim: Anim, group: u16) -> bool {
    let first = Anim::BOTH_T1_BR__R.index() + group * 77;
    (first..first + 42).contains(&anim.index())
}

/// The rig's combo played in the sim: the same animations, timed alike.
fn combos(
    style: Style,
    world: &dyn CollisionWorld,
    anims: &AnimTable,
    rig: Option<&(Gla, AnimationConfig)>,
    report: &mut Report,
) -> serde_json::Value {
    let (names, steps) = combo(style);
    let mut duel = duelist(style, world, anims);
    let from = duel.starts.len();
    for (keys, until) in steps {
        duel.until(4000, keys, |duel| duel.fighter.saber.current_move == until);
    }
    duel.settle();
    let starts = duel.since(from).to_vec();
    let label = format!("{} combo", style.name());
    report.section(&format!("{label} (the rig's)"), &starts);
    let played: Vec<&str> = starts
        .iter()
        .filter(|start| start.mv != Move::READY)
        .map(|start| start.torso.name())
        .collect();
    report.check(
        format!("{label}: the rig's animations"),
        played == names.iter().map(String::as_str).collect::<Vec<_>>(),
        played.join(", "),
    );
    let rows = timing(&label, style, &starts, anims, rig, report);
    serde_json::json!({ "style": style.name(), "moves": rows })
}

/// What a special starts: a move, or (the aerial cartwheels, which set
/// none) an animation on the legs.
#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Move(Move),
    Legs(Anim),
}

impl Expect {
    fn came(self, duel: &Duel) -> bool {
        match self {
            Expect::Move(mv) => duel.fighter.saber.current_move == mv,
            Expect::Legs(anim) => duel.ps.legs.anim == anim,
        }
    }

    fn found(self, start: &Start) -> bool {
        match self {
            Expect::Move(mv) => start.mv == mv,
            Expect::Legs(anim) => start.legs == anim,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Expect::Move(mv) => mv.name(),
            Expect::Legs(anim) => anim.name(),
        }
    }
}

/// A special: its name, the style it is tried in, how it is set up, the
/// keys that start it, what it starts, and its cost in percent of the
/// pool.
struct Special {
    name: &'static str,
    style: Style,
    setup: fn(&mut Duel),
    keys: Keys,
    expect: Expect,
    cost: i32,
}

fn nothing(_: &mut Duel) {}

fn behind(duel: &mut Duel) {
    duel.stand_by(-50.0, 0.0, Anim::BOTH_STAND1);
}

fn lying_ahead(duel: &mut Duel) {
    duel.stand_by(70.0, 0.0, Anim::BOTH_KNOCKDOWN1);
}

fn both_sides(duel: &mut Duel) {
    duel.stand_by(0.0, 50.0, Anim::BOTH_STAND1);
    duel.stand_by(0.0, -50.0, Anim::BOTH_STAND1);
}

fn front_and_back(duel: &mut Duel) {
    duel.stand_by(50.0, 0.0, Anim::BOTH_STAND1);
    duel.stand_by(-50.0, 0.0, Anim::BOTH_STAND1);
}

/// Crouched, then the keys (the lunge and the spins need the crouch).
fn crouched(duel: &mut Duel) {
    duel.hold(200, Keys::default().up(-127));
}

/// A swing let go: the return plays when the kick is asked.
fn returning(duel: &mut Duel) {
    duel.until(3000, Keys::press(ATTACK), |duel| {
        duel.fighter.saber.current_move.pure_attack()
    });
    duel.until(3000, Keys::default(), |duel| {
        duel.fighter.saber.current_move.ret()
    });
}

/// Running forward, then crouched into a forward roll.
fn rolling(duel: &mut Duel) {
    duel.hold(600, Keys::default().with(127, 0));
    duel.until(500, Keys::default().with(127, 0).up(-127), |duel| {
        duel.ps.legs.anim == Anim::BOTH_ROLL_F
    });
}

fn specials() -> Vec<Special> {
    use Expect::{Legs, Move as M};
    let attack = Keys::press(ATTACK);
    let jump = attack.up(127);
    let kata = Keys::press(ATTACK | ALT);
    let kick = Keys::press(ALT);
    let [fast, medium, strong, dual, staff] = STYLES;
    let special = |name, style, setup: fn(&mut Duel), keys, expect, cost| Special {
        name,
        style,
        setup,
        keys,
        expect,
        cost,
    };
    let lunge = attack.with(127, 0).up(-127);
    let mut list = vec![
        special("lunge", fast, crouched, lunge, M(Move::A_LUNGE), 25),
        special("roll stab", fast, rolling, attack, M(Move::ROLL_STAB), 25),
        special(
            "back stab",
            fast,
            behind,
            attack.with(-127, 0),
            M(Move::A_BACKSTAB),
            0,
        ),
        special("kata", fast, nothing, kata, M(Move::A1_SPECIAL), 50),
        special(
            "flip",
            medium,
            nothing,
            jump.with(127, 0),
            M(Move::A_FLIP_SLASH),
            25,
        ),
        special(
            "back attack",
            medium,
            behind,
            attack.with(-127, 0),
            M(Move::A_BACK),
            0,
        ),
        special(
            "crouched back attack",
            medium,
            behind,
            attack.with(-127, 0).up(-127),
            M(Move::A_BACK_CR),
            0,
        ),
        special("kata", medium, nothing, kata, M(Move::A2_SPECIAL), 50),
        special(
            "death from above",
            strong,
            nothing,
            jump.with(127, 0),
            M(Move::A_JUMP_T__B_),
            25,
        ),
        special(
            "back attack",
            strong,
            behind,
            attack.with(-127, 0),
            M(Move::A_BACK),
            0,
        ),
        special("kata", strong, nothing, kata, M(Move::A3_SPECIAL), 50),
        special(
            "jump attack",
            dual,
            nothing,
            jump.with(127, 0),
            M(Move::JUMPATTACK_DUAL),
            25,
        ),
        special("spin", dual, crouched, lunge, M(Move::SPINATTACK_DUAL), 25),
        special("kata", dual, nothing, kata, M(Move::DUAL_SPIN_PROTECT), 50),
        special(
            "mirror, sides",
            dual,
            both_sides,
            attack.with(0, 127),
            M(Move::DUAL_LR),
            0,
        ),
        special(
            "mirror, front and back",
            dual,
            front_and_back,
            attack.with(127, 0),
            M(Move::DUAL_FB),
            0,
        ),
        special(
            "back attack",
            dual,
            behind,
            attack.with(-127, 0),
            M(Move::A_BACK),
            0,
        ),
        special(
            "jump attack",
            staff,
            nothing,
            jump.with(127, 0),
            M(Move::JUMPATTACK_STAFF_RIGHT),
            25,
        ),
        special(
            "back-flip attack",
            staff,
            nothing,
            jump.with(-127, 0),
            M(Move::A_BACKFLIP_ATK),
            25,
        ),
        special("spin", staff, crouched, lunge, M(Move::SPINATTACK), 25),
        special("kata", staff, nothing, kata, M(Move::STAFF_SOULCAL), 50),
        special(
            "butterfly right",
            staff,
            nothing,
            jump.with(0, 127),
            M(Move::BUTTERFLY_RIGHT),
            10,
        ),
        special(
            "butterfly left",
            staff,
            nothing,
            jump.with(0, -127),
            M(Move::BUTTERFLY_LEFT),
            10,
        ),
        special(
            "back stab",
            staff,
            behind,
            attack.with(-127, 0),
            M(Move::A_BACKSTAB),
            0,
        ),
        special(
            "kick forward",
            staff,
            nothing,
            kick.with(127, 0),
            M(Move::KICK_F),
            0,
        ),
        special(
            "kick back",
            staff,
            nothing,
            kick.with(-127, 0),
            M(Move::KICK_B),
            0,
        ),
        special(
            "kick right",
            staff,
            nothing,
            kick.with(0, 127),
            M(Move::KICK_R),
            0,
        ),
        special(
            "kick left",
            staff,
            nothing,
            kick.with(0, -127),
            M(Move::KICK_L),
            0,
        ),
        special(
            "kick out of a return",
            staff,
            returning,
            kick.with(0, 127),
            M(Move::KICK_R),
            0,
        ),
    ];
    for style in [fast, medium, strong, dual] {
        list.push(special(
            "cartwheel right",
            style,
            nothing,
            jump.with(0, 127),
            Legs(Anim::BOTH_ARIAL_RIGHT),
            10,
        ));
        list.push(special(
            "cartwheel left",
            style,
            nothing,
            jump.with(0, -127),
            Legs(Anim::BOTH_ARIAL_LEFT),
            10,
        ));
    }
    for (style, stab) in [
        (fast, Move::STABDOWN),
        (medium, Move::STABDOWN),
        (strong, Move::STABDOWN),
        (dual, Move::STABDOWN_DUAL),
        (staff, Move::STABDOWN_STAFF),
    ] {
        list.push(special(
            "stab down",
            style,
            lying_ahead,
            attack.with(127, 0),
            M(stab),
            25,
        ));
    }
    list
}

/// Each special once, from the ready stance with a full pool: it starts,
/// costs JA's share of the pool, and holds as long as the rig's.
fn try_specials(
    world: &dyn CollisionWorld,
    anims: &AnimTable,
    rig: Option<&(Gla, AnimationConfig)>,
    report: &mut Report,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for special in specials() {
        let mut duel = duelist(special.style, world, anims);
        (special.setup)(&mut duel);
        let from = duel.starts.len();
        let fp = duel.fighter.fp;
        let expect = special.expect;
        let came = duel.until(3000, special.keys, |duel| expect.came(duel));
        let paid = fp - duel.fighter.fp;
        duel.clear();
        duel.settle();
        let starts = duel.since(from).to_vec();
        let label = format!("{} {}", special.style.name(), special.name);
        report.section(&label, &starts);
        let found = starts.iter().find(|start| expect.found(start));
        report.check(
            format!("{label}: starts {}", expect.name()),
            came && found.is_some(),
            found.map_or("not started".to_string(), |start| {
                format!("{} at {} ms", start.torso.name(), start.at)
            }),
        );
        report.check(
            format!("{label}: costs {}% of the pool", special.cost),
            paid == special.cost * 100,
            format!("{:.2} FP of 100", f64::from(paid) / 100.0),
        );
        if let Some(start) = found
            && matches!(expect, Expect::Move(_))
        {
            let timed = timing(
                &label,
                special.style,
                std::slice::from_ref(start),
                anims,
                rig,
                report,
            );
            rows.push(serde_json::json!({
                "special": label, "starts": expect.name(), "paid": paid, "timing": timed,
            }));
        }
    }
    rows
}

/// A special the pool cannot pay for does not start, and says so (JA's
/// `EV_NOAMMO`); a character without the Force pays from its stamina (I22
/// C7), and with an empty bar swings plainly.
fn refusals(world: &dyn CollisionWorld, anims: &AnimTable, report: &mut Report) {
    let mut duel = duelist(Style::Medium, world, anims);
    duel.fighter.fp = 4999;
    duel.events = 0;
    duel.hold(400, Keys::press(ATTACK | ALT));
    let kata = duel.starts.iter().any(|start| start.mv == Move::A2_SPECIAL);
    report.check(
        "a kata with 49.99 of 100 FP is refused",
        !kata && duel.events & events::NO_POWER != 0 && duel.fighter.fp == 4999,
        format!(
            "kata {}, no-power event {}",
            if kata { "started" } else { "not started" },
            duel.events & events::NO_POWER != 0
        ),
    );
    for (bar, what) in [
        (None, "without the Force the lunge is paid from stamina"),
        (
            Some(0),
            "without the Force and an empty bar no lunge, a plain swing",
        ),
    ] {
        let sheet = jedi(weapons_for(Style::Fast), 0);
        let mut duel = Duel::new(&sheet, world, anims);
        let full = i32::from(sheet.pool()) * 100;
        if let Some(bar) = bar {
            duel.fighter.fp = bar;
        }
        duel.light();
        duel.take_style(Style::Fast);
        duel.settle();
        duel.hold(200, Keys::default().up(-127));
        duel.hold(600, Keys::press(ATTACK).with(127, 0).up(-127));
        let lunged = duel.starts.iter().any(|start| start.mv == Move::A_LUNGE);
        let swung = duel.starts.iter().any(|start| start.mv.pure_attack());
        let pass = match bar {
            None => lunged && duel.fighter.fp < full,
            Some(_) => !lunged && swung,
        };
        report.check(
            what,
            pass,
            format!(
                "lunge {lunged}, swing {swung}, {:.2} of {} stamina left",
                f64::from(duel.fighter.fp) / 100.0,
                sheet.pool()
            ),
        );
    }
}

/// When the legs first play `anim` after `from` (ms), if they do.
fn legs_play(duel: &mut Duel, anim: Anim, keys: Keys, within: u64) -> Option<u64> {
    duel.until(within, keys, |duel| duel.ps.legs.anim == anim)
        .then_some(duel.now)
}

/// Knockdowns and getting up (rulebook 10.1): down 1100 ms, then a side
/// key rolls up at once, jump gets up with the Force (500 ms; from jump
/// level 2 the acrobatic get-up), and after
/// 4 s without keys the plain get-up (1 s); the weapon rises 250 ms after.
fn knockdowns(world: &dyn CollisionWorld, anims: &AnimTable, report: &mut Report) {
    const DOWN: u64 = 1100;
    let fresh = || {
        let mut duel = duelist(Style::Medium, world, anims);
        let now = duel.now;
        duel.fighter.saber.knock_down(now, DOWN);
        duel
    };
    let mut duel = fresh();
    let down = duel.now;
    duel.step(Keys::default());
    let (legs, torso) = (duel.ps.legs.anim, duel.ps.torso.anim);
    let lying = legs == Anim::BOTH_KNOCKDOWN1
        && torso == Anim::BOTH_KNOCKDOWN1
        && duel.fighter.saber.current_move == Move::NONE;
    let getup = legs_play(&mut duel, Anim::BOTH_GETUP1, Keys::default(), 8000);
    let got_up = duel.until(3000, Keys::default(), |duel| {
        !duel.fighter.saber.floored()
    });
    let up = duel.now;
    let raised = duel.until(1000, Keys::default(), |duel| {
        duel.fighter.saber.change == sim::combat::saber::Change::None && duel.fighter.weapon_wait_ms <= 0
    });
    let at = getup.map_or(0, |at| at - down);
    outln!(
        "  knocked down at {down} ms for {DOWN} ms: plain get-up {at} ms after it, up at {} ms, the weapon raised at {} ms",
        up - down,
        duel.now - down
    );
    report.check(
        "knocked down: the knockdown on both parts, no move",
        lying,
        format!("legs {}, torso {}", legs.name(), torso.name()),
    );
    // Up at the first command more than 4 s after the time is out.
    report.check(
        "no keys: the plain get-up 4 s after the time is out",
        getup.is_some() && at > DOWN + LIE_MS && at <= DOWN + LIE_MS + u64::from(MSEC),
        format!("{at} ms after the knockdown"),
    );
    let (lasted, rose) = (up - down - at, duel.now - up);
    report.check(
        "no keys: up 1 s later, the weapon raised 250 ms after",
        // The raise's first 8 ms run in the command that gets it up.
        got_up && raised && (1000..1016).contains(&lasted) && (242..=250).contains(&rose),
        format!("the get-up {lasted} ms, the raise {rose} ms"),
    );

    let mut duel = fresh();
    let down = duel.now;
    duel.idle(DOWN);
    let roll = legs_play(
        &mut duel,
        Anim::BOTH_GETUP_BROLL_R,
        Keys::default().with(0, 127),
        400,
    );
    report.check(
        "a side key: the roll get-up once the time is out",
        roll.is_some_and(|at| at - down <= DOWN + 2 * u64::from(MSEC)),
        format!(
            "{} at {} ms",
            Anim::BOTH_GETUP_BROLL_R.name(),
            roll.map_or(0, |at| at - down)
        ),
    );

    // Jump gets up with the Force; from jump level 2 the acrobatic get-up.
    for (level, anim, getup, label) in [
        (
            1,
            Anim::BOTH_FORCE_GETUP_B1,
            Getup::Force,
            "jump at jump level 1: the force get-up, 500 ms",
        ),
        (
            2,
            Anim::BOTH_FORCE_GETUP_B3,
            Getup::Quick,
            "jump at jump level 2: the acrobatic get-up, 500 ms",
        ),
    ] {
        let mut duel = fresh();
        duel.kit.force_jump = level;
        let down = duel.now;
        duel.idle(DOWN);
        let played = legs_play(&mut duel, anim, Keys::default().up(127), 400);
        let from = duel.now;
        let raised = duel.until(1000, Keys::default(), |duel| {
            !duel.fighter.saber.floored()
        });
        report.check(
            label,
            played.is_some()
                && raised
                && duel.fighter.saber.getup == getup
                && (500..500 + 2 * u64::from(MSEC)).contains(&(duel.now - from)),
            format!(
                "{} at {} ms, up {} ms later",
                anim.name(),
                played.map_or(0, |at| at - down),
                duel.now - from
            ),
        );
    }
}

/// JA's wait before a knocked-down character gets up by itself.
const LIE_MS: u64 = 4000;

/// Changing hands (rulebook 11): the pistol goes down (200 ms), the saber
/// is drawn unlit and raised (250 ms); the first attack lights it; no
/// change in the middle of a move; back to the pistol, the saber goes out.
fn weapon_changes(world: &dyn CollisionWorld, anims: &AnimTable, report: &mut Report) {
    let sheet = jedi([Some(pistol()), Some(saber(8))], 100);
    let mut duel = Duel::new(&sheet, world, anims);
    let from = duel.starts.len();
    let to_saber = Keys {
        weapon: 1,
        ..Keys::default()
    };
    duel.step(to_saber);
    let asked = duel.now;
    let dropping = duel.ps.torso.anim == Anim::TORSO_DROPWEAP1;
    duel.until(1000, to_saber, |duel| duel.fighter.hand == 1);
    let drawn = duel.now - asked;
    let draw_anim = duel.ps.torso.anim;
    let unlit = !duel.fighter.saber.on;
    duel.until(3000, to_saber, |duel| {
        duel.fighter.weapon_wait_ms <= 0 && duel.fighter.saber.change == sim::combat::saber::Change::None
    });
    let raised = duel.now - asked;
    duel.events = 0;
    duel.step(Keys {
        buttons: ATTACK,
        ..to_saber
    });
    let lit = duel.fighter.saber.on && duel.events & events::SABER_ON != 0;
    // Asking for the pistol mid-swing changes nothing until the swing ends.
    duel.until(
        2000,
        Keys {
            buttons: ATTACK,
            ..to_saber
        },
        |duel| duel.fighter.saber.current_move.pure_attack(),
    );
    let mid = duel.fighter.saber.current_move;
    duel.hold(80, Keys::default());
    let kept =
        duel.fighter.hand == 1 && duel.fighter.saber.change == sim::combat::saber::Change::None;
    duel.until(5000, Keys::default(), |duel| duel.fighter.hand == 0);
    let back = duel.fighter.hand == 0 && !duel.fighter.saber.on;
    report.section("weapon changes: pistol to saber and back", duel.since(from));
    outln!(
        "    down {dropping}; drawn at {drawn} ms with {}; raised at {raised} ms; lit by attack {lit}",
        draw_anim.name()
    );
    report.check(
        "to the saber: down 200 ms, drawn unlit",
        dropping && drawn == 200 && draw_anim == Anim::BOTH_STAND1TO2 && unlit,
        format!(
            "drawn after {drawn} ms playing {}, unlit {unlit}",
            draw_anim.name()
        ),
    );
    report.check(
        "the draw raises the weapon 250 ms, the animation holds longer",
        raised >= 450,
        format!("ready {raised} ms after asking"),
    );
    report.check("the first attack lights it", lit, format!("lit {lit}"));
    report.check(
        "no change of hands in a swing",
        kept,
        format!("asked during {}, still in hand {kept}", mid.name()),
    );
    report.check(
        "back to the pistol: the saber goes out",
        back,
        format!("hand {}, lit {}", duel.fighter.hand, duel.fighter.saber.on),
    );
}

/// The style button (JA's `saberAttackCycle`): a single saber goes up
/// through the styles its feats open and wraps; pressed mid-swing, the
/// change waits for the swing; a pair puts its second saber out and fights
/// fast, then back.
fn cycling(world: &dyn CollisionWorld, anims: &AnimTable, report: &mut Report) {
    let mut duel = duelist(Style::Medium, world, anims);
    let mut seen = vec![duel.fighter.saber.style];
    for _ in 0..3 {
        duel.step(Keys::press(buttons::SABER_STYLE));
        duel.step(Keys::default());
        seen.push(duel.fighter.saber.style);
    }
    let names: Vec<&str> = seen
        .iter()
        .map(|style| style.map_or("-", Style::name))
        .collect();
    report.check(
        "the style button: medium, strong, fast, medium",
        seen == [
            Some(Style::Medium),
            Some(Style::Strong),
            Some(Style::Fast),
            Some(Style::Medium),
        ],
        names.join(", "),
    );
    duel.until(2000, Keys::press(ATTACK), |duel| {
        duel.fighter.saber.current_move.pure_attack()
    });
    duel.step(Keys::press(buttons::SABER_STYLE | ATTACK));
    let queued = duel.fighter.saber.queued;
    let during = duel.fighter.saber.style;
    // Applied at the first command with the weapon timer out.
    duel.settle();
    duel.step(Keys::default());
    report.check(
        "pressed mid-swing, it waits for the swing",
        queued == Some(Style::Strong)
            && during == Some(Style::Medium)
            && duel.fighter.saber.style == Some(Style::Strong),
        format!(
            "queued {}, during {}, after {}",
            queued.map_or("-", Style::name),
            during.map_or("-", Style::name),
            duel.fighter.saber.style.map_or("-", Style::name)
        ),
    );
    // Two blades always fight dual (the owner's rule): the key leaves a pair
    // as it is, and a second blade put in hand (the equip screen's) or
    // taken away sets the style with no key.
    let mut duel = duelist(Style::Dual, world, anims);
    duel.step(Keys::press(buttons::SABER_STYLE));
    duel.step(Keys::default());
    let keyed = (duel.fighter.saber.style, duel.fighter.saber.second_off);
    let mut duel = duelist(Style::Medium, world, anims);
    duel.kit = Kit::of(&jedi(weapons_for(Style::Dual), 100));
    duel.step(Keys::default());
    let paired = duel.fighter.saber.style;
    duel.kit = Kit::of(&jedi(weapons_for(Style::Medium), 100));
    duel.step(Keys::default());
    let single = duel.fighter.saber.style;
    report.check(
        "the pair: dual whatever the key; a second blade in hand fights dual, taken away single",
        keyed == (Some(Style::Dual), false)
            && paired == Some(Style::Dual)
            && single == Some(Style::Medium),
        format!("keyed {keyed:?}; second blade {paired:?}; taken away {single:?}"),
    );
    // A staff's key puts its second blade out, fighting as a single blade
    // (its stance and style), then back, fighting as a staff.
    let mut duel = duelist(Style::Staff, world, anims);
    duel.step(Keys::press(buttons::SABER_STYLE));
    duel.step(Keys::default());
    let half = (duel.fighter.saber.style, duel.fighter.saber.second_off);
    duel.settle();
    let half_stance = duel.ps.legs.anim;
    duel.step(Keys::press(buttons::SABER_STYLE));
    duel.step(Keys::default());
    let whole = (duel.fighter.saber.style, duel.fighter.saber.second_off);
    report.check(
        "the staff: one blade out fights single, both a staff",
        half == (Some(Style::Medium), true)
            && half_stance == Anim::BOTH_STAND2
            && whole == (Some(Style::Staff), false),
        format!("one blade {half:?} ({half_stance:?}), both {whole:?}"),
    );
}

/// Every animation the move table plays in the styles, found in the
/// skeleton's `animation.cfg`.
fn coverage(anims: &AnimTable, report: &mut Report) {
    let missing: Vec<&str> = Anim::ALL
        .iter()
        .filter(|anim| anim.index() >= Anim::BOTH_SABERFAST_STANCE.index())
        .filter(|anim| !sim::combat::saber::lock::NEVER_PLAYED.contains(anim))
        .filter(|anim| anims.get(**anim).frames == 0)
        .map(|anim| anim.name())
        .collect();
    report.check(
        "every saber animation is in animation.cfg",
        missing.is_empty(),
        if missing.is_empty() {
            "all found".to_string()
        } else {
            format!("missing {}", missing.join(", "))
        },
    );
}

/// `kjk combat saber`.
pub fn saber_moves(discovery: &Discovery) -> ExitCode {
    let (anims, note) = crate::stage::anims(discovery);
    let rig = crate::stage::ja_skeleton(discovery).ok();
    let level = sim::Level::arena();
    let world = level.collision().as_ref();
    outln!("combat saber: {note}");
    if rig.is_none() {
        outln!("  (no JA skeleton: the rig's timelines are not compared)");
    }
    let mut report = Report::default();
    coverage(&anims, &mut report);
    let mut combos_facts = Vec::new();
    for style in STYLES {
        outln!("{} style", style.name());
        holds(style, world, &anims, &mut report);
        combos_facts.push(combos(style, world, &anims, rig.as_ref(), &mut report));
    }
    outln!("specials");
    let specials = try_specials(world, &anims, rig.as_ref(), &mut report);
    refusals(world, &anims, &mut report);
    outln!("knockdowns");
    knockdowns(world, &anims, &mut report);
    outln!("weapons and styles");
    weapon_changes(world, &anims, &mut report);
    cycling(world, &anims, &mut report);
    let failed: Vec<&Check> = report.checks.iter().filter(|check| !check.pass).collect();
    outln!(
        "combat saber: {} checks, {} failed",
        report.checks.len(),
        failed.len()
    );
    for check in &failed {
        outln!("  FAIL {}: {}", check.name, check.seen);
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat saber", "anims": note, "rig": rig.is_some(),
        "checks": report.checks.iter().map(|check| serde_json::json!({
            "name": check.name, "pass": check.pass, "seen": check.seen,
        })).collect::<Vec<_>>(),
        "combos": combos_facts, "specials": specials, "sections": report.sections,
    }));
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
