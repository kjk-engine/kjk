//! The probes of a chosen feat attack (`combat::picks`), on the proving
//! ground, no install:
//! - the next-feat action moves a gun round the lines known, its own first,
//!   and its alternate takes the choice; each gun type keeps its own;
//! - one line known leaves nothing to cycle; a single blade has no choice;
//! - two blades and a double-bladed weapon carry none until one is chosen,
//!   each known line in turn, then none, each its own; only the lines known;
//! - a blow carries the chosen line's feat, and only Flurry's counts more pace;
//! - a script's talent chooses its line, one known, one the weapon takes;
//! - the choices come back from a snapshot, and from an old one as none;
//! - the action comes back whole from its bytes.

use super::SimWorld;
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::picks::{Melee, Picks, Shown};
use crate::combat::saber::{Kit, Style};
use crate::combat::sheet::{Blaster, Delivery, SaberShape, Weapon, feats};
use crate::combat::specials::Special;
use crate::input::{Command, Seat, TickInput};
use crate::probe::{Checks, check};
use crate::wire::{Reader, Writer};
use crate::{StepReason, WorldConfig};

/// The dev soldier's weapon, as `delivery` in the hands `hands`.
fn holding(delivery: Delivery, hands: usize, known: &[u16]) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    let mut weapon: Weapon = sheet.main_weapon();
    weapon.delivery = delivery;
    weapon.proficiency = None;
    sheet.weapons = [Some(weapon), (hands > 1).then_some(weapon)];
    sheet.feats = known.iter().fold(sheet.feats, |rows, row| rows.with(*row));
    sheet
}

const GUN_LINES: [u16; 3] = [
    feats::POWER_BLAST[0],
    feats::RAPID_SHOT[0],
    feats::SNIPER_SHOT[0],
];
const BLADE_LINES: [u16; 3] = [
    feats::POWER_ATTACK[0],
    feats::CRITICAL_STRIKE[0],
    feats::FLURRY[0],
];

fn dual(known: &[u16]) -> Sheet {
    holding(Delivery::Saber(SaberShape::Single), 2, known)
}

fn staff(known: &[u16]) -> Sheet {
    holding(Delivery::Saber(SaberShape::Staff), 1, known)
}

fn pistol(known: &[u16]) -> Sheet {
    holding(Delivery::Blaster(Blaster::Pistol), 1, known)
}

/// The next-feat action, through the world: the special after each press.
fn gun_cycle() -> Result<String, String> {
    // Seats' actions run with the scripts' step: a library, here an empty one.
    let library = crate::probe::library::<String>("void ActionWait(float fSeconds);", []);
    let level = crate::determinism::proving_ground().with_scripts(library);
    let config = WorldConfig {
        sheet: pistol(&GUN_LINES),
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let mut seen = Vec::new();
    for tick in 1..=4 {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        if tick > 1 {
            input.act(Seat(0), Action::NextFeat { hand: 0 });
        }
        let snapshot = crate::step(&mut world, tick, &input, 8, StepReason::Authority);
        let sheet = snapshot.body(Seat(0)).and_then(|body| body.sheet);
        seen.push(sheet.and_then(|sheet| Special::of(&sheet, Blaster::Pistol)));
    }
    let want = [
        Special::PowerBlast(1),
        Special::RapidShot(1),
        Special::SniperShot(1),
        Special::PowerBlast(1),
    ]
    .map(Some);
    check(seen == want, format!("{seen:?}"))
}

/// Each gun type keeps its own choice; a line lost falls back to the gun's.
fn gun_types_apart() -> Result<String, String> {
    let mut sheet = pistol(&GUN_LINES);
    Picks::next(&mut sheet, 0);
    let pistol_now = Special::of(&sheet, Blaster::Pistol);
    let rifle_now = Special::of(&sheet, Blaster::Rifle);
    let mut forgot = sheet;
    forgot.feats = pistol(&[GUN_LINES[0]]).feats;
    let fell_back = Special::of(&forgot, Blaster::Pistol);
    check(
        pistol_now == Some(Special::RapidShot(1))
            && rifle_now == Some(Special::RapidShot(1))
            && fell_back == Some(Special::PowerBlast(1)),
        format!("pistol {pistol_now:?}, rifle {rifle_now:?}, line lost {fell_back:?}"),
    )
}

/// Nothing to cycle: one gun line known, or a single blade whatever it knows.
fn nothing_to_cycle() -> Result<String, String> {
    let mut one = pistol(&[GUN_LINES[1]]);
    let mut single = holding(Delivery::Saber(SaberShape::Single), 1, &BLADE_LINES);
    let moved = [Picks::next(&mut one, 0), Picks::next(&mut single, 0)];
    let shown = [Picks::shown(&one, 0), Picks::shown(&single, 0)];
    check(
        moved == [false, false] && shown == [None, None],
        format!("moved {moved:?}, shown {shown:?}"),
    )
}

/// The blade lines in turn, then none; each weapon's its own.
fn blade_cycle() -> Result<String, String> {
    let lines = |make: fn(&[u16]) -> Sheet, known: &[u16], at: usize| {
        let mut sheet = make(known);
        let mut seen = vec![sheet.picks.blades()[at]];
        for _ in 0..4 {
            Picks::next(&mut sheet, 0);
            seen.push(sheet.picks.blades()[at]);
        }
        seen
    };
    let every = [
        Melee::None,
        Melee::Power,
        Melee::Critical,
        Melee::Flurry,
        Melee::None,
    ];
    let (pair, two) = (lines(dual, &BLADE_LINES, 0), lines(staff, &BLADE_LINES, 1));
    let some = lines(dual, &BLADE_LINES[1..2], 0);
    let some_want = [
        Melee::None,
        Melee::Critical,
        Melee::None,
        Melee::Critical,
        Melee::None,
    ];
    let mut apart = dual(&BLADE_LINES);
    Picks::next(&mut apart, 0);
    let none_known = lines(dual, &[], 0);
    check(
        pair == every
            && two == every
            && some == some_want
            && apart.picks.blades()[1] == Melee::None
            && none_known == [Melee::None; 5],
        format!("dual {pair:?}, staff {two:?}, critical alone {some:?}, none known {none_known:?}"),
    )
}

/// A blow carries the line chosen; only Flurry's counts more pace.
fn blows_carry() -> Result<String, String> {
    let chosen = |presses: usize, style: Style, make: fn(&[u16]) -> Sheet| {
        let mut sheet = make(&BLADE_LINES);
        for _ in 0..presses {
            Picks::next(&mut sheet, 0);
        }
        let kit = Kit::of(&sheet);
        (
            kit.swing(style, sheet.main_weapon()),
            kit.paced(style, 1000),
            sheet.main_weapon(),
        )
    };
    let mut wrong = Vec::new();
    use rules::saber_moves::styles;
    for (style, make) in [
        (Style::Dual, dual as fn(&[u16]) -> Sheet),
        (Style::Staff, staff),
    ] {
        let (none, none_pace, base) = chosen(0, style, make);
        let (power, power_pace, _) = chosen(1, style, make);
        let (critical, critical_pace, _) = chosen(2, style, make);
        let (flurry, flurry_pace, _) = chosen(3, style, make);
        let right = none == base
            && none_pace == 1000
            && power.damage_bonus == base.damage_bonus + styles::POWER_ATTACK[0]
            && power.attack_bonus == base.attack_bonus - styles::POWER_ATTACK_PENALTY
            && power_pace == 1000
            && critical.crit_threat == base.crit_threat.max(1) * styles::CRITICAL_THREAT
            && critical_pace == 1000
            && flurry.attack_bonus == base.attack_bonus - styles::FLURRY_PENALTY[0]
            && flurry_pace == styles::FLURRY_PACE;
        if !right {
            wrong.push(style.name());
        }
    }
    let single = Kit {
        flurry: 1,
        ..Kit::default()
    };
    let unfeated = Kit::default();
    check(
        wrong.is_empty()
            && single.paced(Style::Fast, 1000) == styles::FLURRY_PACE
            && single.paced(Style::Medium, 1000) == 1000
            && unfeated.paced(Style::Fast, 1000) == 1000,
        format!("wrong: {wrong:?}"),
    )
}

/// A script's talent chooses a line the weapon takes and the sheet holds,
/// if none is chosen: a choice made stands, and a gun's own line needs none.
fn talent_chooses() -> Result<String, String> {
    let mut blades = dual(&BLADE_LINES[..1]);
    let taken = Picks::choose(&mut blades, 0, BLADE_LINES[0]);
    let again = Picks::choose(&mut blades, 0, BLADE_LINES[0]);
    let unknown = Picks::choose(&mut blades, 0, BLADE_LINES[2]);
    let gun_row = Picks::choose(&mut blades, 0, GUN_LINES[1]);
    let mut gun = pistol(&GUN_LINES);
    let own_line = Picks::choose(&mut gun, 0, GUN_LINES[0]);
    let gun_taken = Picks::choose(&mut gun, 0, GUN_LINES[2]);
    let overruled = Picks::choose(&mut gun, 0, GUN_LINES[1]);
    let mut player_chose = dual(&BLADE_LINES);
    Picks::next(&mut player_chose, 0);
    let blade_overruled = Picks::choose(&mut player_chose, 0, BLADE_LINES[2]);
    let mut single = holding(Delivery::Saber(SaberShape::Single), 1, &BLADE_LINES);
    let single_taken = Picks::choose(&mut single, 0, BLADE_LINES[0]);
    check(
        taken
            && !again
            && !unknown
            && !gun_row
            && blades.picks.blades()[0] == Melee::Power
            && !own_line
            && gun_taken
            && !overruled
            && !blade_overruled
            && Special::of(&gun, Blaster::Pistol) == Some(Special::SniperShot(1))
            && !single_taken,
        format!(
            "taken {taken}, again {again}, unknown {unknown}, gun row {gun_row}, own line {own_line}, gun {gun_taken}, overruled {overruled} {blade_overruled}, single {single_taken}"
        ),
    )
}

/// What the HUD is told.
fn shown() -> Result<String, String> {
    let mut blades = dual(&BLADE_LINES);
    let before = Picks::shown(&blades, 0);
    Picks::next(&mut blades, 0);
    let after = Picks::shown(&blades, 0);
    let gun = Picks::shown(&pistol(&GUN_LINES), 0);
    check(
        before == Some(Shown::Nothing)
            && after == Some(Shown::Feat(BLADE_LINES[0]))
            && gun == Some(Shown::Feat(GUN_LINES[0])),
        format!("before {before:?}, after {after:?}, gun {gun:?}"),
    )
}

/// The choices through their bytes, and the action through its.
fn round_trip() -> Result<String, String> {
    let mut sheet = pistol(&GUN_LINES);
    Picks::next(&mut sheet, 0);
    let mut out = Writer::default();
    sheet.picks.encode(&mut out);
    let got =
        Picks::decode(&mut Reader::new("picks", &out.0), true).map_err(|e| format!("{e:?}"))?;
    let action = Action::NextFeat { hand: 1 };
    let back = Action::from_bytes(&action.to_bytes());
    check(
        got == sheet.picks && back == Ok(action),
        format!("sent {:?}, read {got:?}, action {back:?}", sheet.picks),
    )
}

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 8] = [
        ("picks_gun_cycle", gun_cycle),
        ("picks_gun_types_apart", gun_types_apart),
        ("picks_nothing_to_cycle", nothing_to_cycle),
        ("picks_blade_cycle", blade_cycle),
        ("picks_blows_carry", blows_carry),
        ("picks_talent_chooses", talent_chooses),
        ("picks_hud_shown", shown),
        ("picks_round_trip", round_trip),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}
