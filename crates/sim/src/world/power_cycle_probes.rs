//! The probes of what the power key and the next-feat key cycle through
//! (issue 417), on the proving ground, no install:
//! - a power from the character's own menu that Jedi Academy has no shape
//!   for (TSL's Force Barrier), picked at a level-up, is among the powers
//!   the key selects, and the key casts it through its impact script, paid;
//! - a power past row 255 (TSL's Breath Control, row 270) is selected and
//!   cast the same way, through the command's two bytes;
//! - a power from a foe's menu with no shape (Crush Opposition) is selected
//!   too, and a known power that no menu offers (a passive one) is not;
//! - fists and a stun baton cycle the melee feat lines known, Power Attack
//!   first, and the blow carries the line chosen; a script's talent chooses
//!   one; the choice comes back from its bytes, and from old bytes as none.

use super::{Fighting, Id, PlayerSheet, SimWorld};
use crate::combat::Sheet;
use crate::combat::force::{self, Power};
use crate::combat::picks::{Melee, Picks, Shown};
use crate::combat::saber::Kit;
use crate::combat::sheet::{Delivery, Weapon, feats};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::levels::{Choices, Class, Classes, SKILLS};
use crate::probe::{Checks, check};
use crate::script::{Menu, Talent, Talents};
use crate::snapshot::{SimId, Snapshot};
use crate::wire::{Reader, Writer};
use crate::{StepReason, WorldConfig};
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;

const MSEC: u32 = 100;

/// TSL's rows (`combat::spells` names only the rows the native rules read).
pub(super) const FORCE_BARRIER: u16 = 135;
const CRUSH_OPPOSITION: u16 = 144;
const PRECOGNITION: u16 = 178;
const BREATH_CONTROL: u16 = 270;
/// Every power here costs this (whole FP).
pub(super) const COST: u16 = 20;

/// Each script-cast row with the menu that offers it and the custom token
/// its impact script sets.
const ROWS: [(u16, Menu, i32); 4] = [
    (FORCE_BARRIER, Menu::Friendly, 40),
    (CRUSH_OPPOSITION, Menu::Hostile, 41),
    (PRECOGNITION, Menu::None, 42),
    (BREATH_CONTROL, Menu::Friendly, 43),
];

const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
"#;

/// An impact script that sets custom token `n` to "1".
fn marks(n: i32) -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(1)))
        .op(Op::Action {
            routine: 1,
            args: 1,
        })
        .op(Op::Const(Const::Int(n)))
        .op(Op::Action {
            routine: 0,
            args: 2,
        });
    asm.op(Op::Retn);
    asm.build()
}

/// The impact script of `row`.
fn impact(row: u16) -> String {
    format!("probe_impact_{row}")
}

/// One Jedi class: each level picks two powers, Force Barrier and Breath
/// Control among those it may.
fn classes() -> Classes {
    let class = Class {
        hit_die: 8,
        force_die: 8,
        skill_points: 4,
        bab: (0..20).collect(),
        saves: vec![[1, 2, 0]; 20],
        defence: vec![0; 20],
        feat_picks: vec![0; 20],
        power_picks: vec![2; 20],
        powers: vec![(1, FORCE_BARRIER), (1, BREATH_CONTROL)],
        skill_order: (0..SKILLS as u8).collect(),
        ..Class::default()
    };
    Classes {
        classes: vec![class],
        levels: vec![0, 1000, 3000, 6000],
        ..Classes::default()
    }
}

/// The proving ground with every row of `ROWS` costing `COST` and its
/// impact script, the rows' menus as `menus` gives them, and one seat
/// with `sheet`.
fn world(sheet: Sheet, menus: impl Fn(Menu) -> Menu) -> Result<SimWorld, String> {
    world_with(sheet, menus, |_| {})
}

/// `world`, its module changed by `change` first.
pub(super) fn world_with(
    sheet: Sheet,
    menus: impl Fn(Menu) -> Menu,
    change: impl FnOnce(&mut crate::module::ModuleData),
) -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let size = usize::from(BREATH_CONTROL) + 1;
    data.force_costs = vec![COST; size];
    let mut spells = vec![Talent::default(); size];
    let mut impacts = vec![String::new(); size];
    for (row, menu, _) in ROWS {
        spells[usize::from(row)].menu = menus(menu);
        impacts[usize::from(row)] = impact(row);
    }
    let scripts = ROWS.map(|(row, _, token)| (impact(row), marks(token)));
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let library = crate::probe::library_with(CATALOG, scripts, talents);
    change(&mut data);
    let level = ground.with_module(data).with_scripts(library);
    let config = WorldConfig {
        sheet,
        ..WorldConfig::one_seat()
    };
    SimWorld::new(config, level).map_err(|error| error.to_string())
}

pub(super) fn step(world: &mut SimWorld, command: Command) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), command);
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// The player's id and fighter in `world` now.
pub(super) fn player(world: &mut SimWorld) -> Option<(SimId, crate::combat::Fighter)> {
    let id = world.snapshot().body(Seat(0))?.id;
    let fighter = world
        .world
        .query::<(&Id, &Fighting)>()
        .iter(&world.world)
        .find(|(of, _)| of.0 == id)
        .map(|(_, fighting)| fighting.0)?;
    Some((id, fighter))
}

/// The player's sheet set to `sheet` with a pool of 100 FP, the pool full.
pub(super) fn become_(world: &mut SimWorld, sheet: Sheet) {
    let sheet = Sheet {
        max_fp: 100,
        ..sheet
    };
    let Some((id, _)) = player(world) else {
        return;
    };
    for (of, mut own) in world
        .world
        .query::<(&Id, &mut PlayerSheet)>()
        .iter_mut(&mut world.world)
    {
        if of.0 == id {
            own.0 = sheet;
        }
    }
    for (of, mut fighting) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if of.0 == id {
            fighting.0.fp = 100 * 100;
        }
    }
}

/// Custom token `n` in `snapshot` ("-" for none).
pub(super) fn token(snapshot: &Snapshot, n: i32) -> String {
    snapshot
        .talk
        .tokens
        .get(&n)
        .cloned()
        .unwrap_or_else(|| "-".into())
}

/// The power key pressed with `row` selected, then a second: the impact's
/// token and the FP paid on the press (hundredths, less the little a step
/// regains).
pub(super) fn cast(world: &mut SimWorld, row: u16, n: i32) -> (String, i32) {
    let before = player(world).map_or(0, |(_, f)| f.fp);
    let held = Command {
        force: row,
        buttons: buttons::FORCE_POWER,
        ..Command::default()
    };
    step(world, held);
    let after = player(world).map_or(0, |(_, f)| f.fp);
    let mut last = step(world, held.idle());
    for _ in 0..10 {
        last = step(world, held.idle());
    }
    (token(&last, n), before - after)
}

/// The dev soldier at level 1 of the probe's class, with its next level
/// taken picking Force Barrier and Breath Control.
fn levelled() -> Result<Sheet, String> {
    let sheet = Sheet {
        level: 1,
        ..Sheet::dev_soldier()
    };
    let choices = Choices {
        class: 0,
        ability: None,
        skills: [0; SKILLS],
        feats: Vec::new(),
        powers: vec![FORCE_BARRIER, BREATH_CONTROL],
    };
    let career = vec![(0, 1)];
    classes()
        .taken(&sheet, &career, &choices, 0)
        .map(|(grown, ..)| grown)
        .map_err(|why| format!("the level-up refused: {why:?}"))
}

/// Force Barrier and Breath Control picked at a level-up: both selectable,
/// both cast by the key through their impact scripts and paid.
fn level_up_powers() -> Result<String, String> {
    let grown = levelled()?;
    let mut world = world(Sheet::dev_soldier(), |menu| menu)?;
    step(&mut world, Command::default());
    become_(&mut world, grown);
    let library = world.level().scripts().cloned();
    let choices = force::choices(&grown.powers, library.as_deref());
    let chosen = [FORCE_BARRIER, BREATH_CONTROL]
        .map(|row| force::selected(&grown.powers, row, true, library.as_deref()).map(|(p, ..)| p));
    let barrier = cast(&mut world, FORCE_BARRIER, 40);
    become_(&mut world, grown);
    let breath = cast(&mut world, BREATH_CONTROL, 43);
    let paid = |fp: i32| (i32::from(COST - 1) * 100..=i32::from(COST) * 100).contains(&fp);
    check(
        choices.contains(&FORCE_BARRIER)
            && choices.contains(&BREATH_CONTROL)
            && chosen == [Some(Power::Scripted); 2]
            && barrier.0 == "1"
            && paid(barrier.1)
            && breath.0 == "1"
            && paid(breath.1),
        format!(
            "choices {choices:?}, selected as {chosen:?}; Force Barrier's impact {:?}, \
             {} hundredths paid; Breath Control's {:?}, {} paid",
            barrier.0, barrier.1, breath.0, breath.1
        ),
    )
}

/// A foe's power with no shape is selected; a known power that no menu
/// offers is not, nor is any without the scripts that cast it.
fn menus_decide() -> Result<String, String> {
    let powers = crate::combat::sheet::Rows::default()
        .with(CRUSH_OPPOSITION)
        .with(PRECOGNITION)
        .with(FORCE_BARRIER);
    let offered = world(Sheet::dev_soldier(), |menu| menu)?;
    let unoffered = world(Sheet::dev_soldier(), |_| Menu::None)?;
    let of = |world: &SimWorld| {
        let library = world.level().scripts().cloned();
        force::choices(&powers, library.as_deref())
    };
    let (with_menus, without) = (of(&offered), of(&unoffered));
    let no_scripts = force::choices(&powers, None);
    check(
        with_menus == [FORCE_BARRIER, CRUSH_OPPOSITION]
            && without.is_empty()
            && no_scripts.is_empty(),
        format!("offered {with_menus:?}; with no menu {without:?}; with no scripts {no_scripts:?}"),
    )
}

/// The dev soldier with nothing in its hands, or a stun baton, knowing
/// `known`.
fn unarmed(baton: bool, known: &[u16]) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    let weapon = baton.then_some(Weapon {
        delivery: Delivery::Baton,
        base_item: 1,
        ..Weapon::UNARMED
    });
    sheet.weapons = [weapon, None];
    sheet.feats = known.iter().fold(sheet.feats, |rows, row| rows.with(*row));
    sheet
}

const LINES: [u16; 3] = [
    feats::POWER_ATTACK[0],
    feats::CRITICAL_STRIKE[0],
    feats::FLURRY[0],
];

/// Fists and a stun baton go round the lines known, then none; the HUD
/// shows the one chosen; the blow carries it.
fn fists_cycle() -> Result<String, String> {
    use rules::saber_moves::styles;
    let mut wrong = Vec::new();
    for baton in [false, true] {
        let mut sheet = unarmed(baton, &LINES);
        let mut seen = vec![sheet.picks.hands()];
        for _ in 0..4 {
            Picks::next(&mut sheet, 0);
            seen.push(sheet.picks.hands());
        }
        let mut power = unarmed(baton, &LINES);
        Picks::next(&mut power, 0);
        let shown = Picks::shown(&power, 0);
        let kit = Kit::of(&power);
        let base = power.main_weapon();
        let blow = kit.carry(power.picks.hands(), base);
        let every = [
            Melee::None,
            Melee::Power,
            Melee::Critical,
            Melee::Flurry,
            Melee::None,
        ];
        let right = seen == every
            && shown == Some(Shown::Feat(LINES[0]))
            && blow.damage_bonus == base.damage_bonus + styles::POWER_ATTACK[0]
            && kit.line_paced(Melee::Flurry, 1000) == styles::FLURRY_PACE;
        if !right {
            wrong.push(format!(
                "baton {baton}: seen {seen:?}, shown {shown:?}, blow {:?} over {:?}",
                blow.damage_bonus, base.damage_bonus
            ));
        }
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}"))
}

/// A script's talent chooses fists' line once; the choice keeps through
/// its bytes, and old bytes (before fists had one) read none.
fn fists_talent_and_bytes() -> Result<String, String> {
    let mut sheet = unarmed(false, &LINES);
    let taken = Picks::choose(&mut sheet, 0, feats::FLURRY[0]);
    let again = Picks::choose(&mut sheet, 0, feats::POWER_ATTACK[0]);
    let mut out = Writer::default();
    sheet.picks.encode(&mut out);
    let back = Picks::decode(&mut Reader::new("picks", &out.0), true, true)
        .map_err(|e| format!("{e:?}"))?;
    let old = &out.0[..out.0.len() - 1];
    let before =
        Picks::decode(&mut Reader::new("picks", old), true, false).map_err(|e| format!("{e:?}"))?;
    check(
        taken
            && !again
            && sheet.picks.hands() == Melee::Flurry
            && back == sheet.picks
            && before.hands() == Melee::None,
        format!(
            "taken {taken}, again {again}, chosen {:?}, read back {:?}, old bytes {:?}",
            sheet.picks.hands(),
            back.hands(),
            before.hands()
        ),
    )
}

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 4] = [
        ("power_cycle_level_up_powers", level_up_powers),
        ("power_cycle_menus_decide", menus_decide),
        ("power_cycle_fists", fists_cycle),
        ("power_cycle_fists_talent_and_bytes", fists_talent_and_bytes),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}
