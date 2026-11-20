//! The probes of the feat line a fighter chooses itself (`kjk sim probes
//! --only "feat choices"`; `combat::choose::feat_line`, `world::minds`).
//! Each case runs on the proving ground with a script library, as the
//! games' own levels have, and needs no install. The player stands still
//! at one end of the room. A fighter stands about 7 m away, or 3 m away
//! for the gunner, and fights the player. In some cases the fighter fights
//! a creature beside the player instead: a companion fights a hostile
//! creature, and an enemy fights a creature with a lightsaber, which lights
//! it in the fight. The cases:
//! - an enemy with a single lightsaber, which knows all three melee lines,
//!   fights in the strong style against a creature with a lit lightsaber;
//! - the same enemy fights in the fast style against a tired player;
//! - the same enemy, starting in the fast style, fights in the medium style
//!   against a player who neither blocks nor is tired;
//! - the same enemy keeps the fast style for 6 seconds after the player
//!   is no longer tired, and then takes the medium style;
//! - the same enemy, when it chooses the medium style while it chains its
//!   swings, takes it as a new move begins and within 2.5 seconds;
//! - a companion with two lightsabers takes Flurry against a tired enemy;
//! - a companion whose line the player picked with the next-feat key keeps
//!   that line against the same enemy;
//! - an enemy with two lightsabers, and an enemy with a pistol, whose line
//!   changed 1 second in, change no line for 6 seconds after that;
//! - an enemy with a pistol takes Rapid Shot against the player 3 m away,
//!   and fires it;
//! - an enemy with a pistol that knows only Power Blast fires it at a
//!   creature with a lit lightsaber.

use super::{Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::combat::picks::{Melee, Picks};
use crate::combat::saber::Style;
use crate::combat::sheet::{Blaster, Delivery, SaberShape, factions, feats};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::party::Npc;
use crate::snapshot::SimId;
use crate::world::picks_probes::holding;
use crate::{StepReason, WorldConfig};

/// A step's milliseconds.
const MSEC: u32 = 50;

/// The first rank of each melee line, and of the two gun lines the gunner
/// knows.
const MELEE: [u16; 3] = [
    feats::POWER_ATTACK[0],
    feats::CRITICAL_STRIKE[0],
    feats::FLURRY[0],
];
const GUN: [u16; 2] = [feats::POWER_BLAST[0], feats::RAPID_SHOT[0]];

/// The hit points of everyone, so that nobody falls during a case.
const HP: u16 = 5000;
/// How long the fights last (sim ms). This is longer than any case.
const FOREVER: u64 = 1_000_000;

const FIGHTER: &str = "probe_fighter";
const FOE: &str = "probe_foe";

/// Where everyone stands (metres in the room's frame).
const PLAYER_AT: (f32, f32) = (-2.0, -4.0);
const FIGHTER_AT: (f32, f32) = (-2.0, 3.0);
const CLOSE_AT: (f32, f32) = (-2.0, -1.0);
const FOE_AT: (f32, f32) = (-1.0, 3.0);

/// What the fighter holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arms {
    Saber,
    TwoSabers,
    Pistol,
    /// A pistol, knowing only Power Blast.
    BlastPistol,
}

/// A case. It says what the fighter holds, whether it is a companion,
/// whether the player picked its line, the style it starts in, whether its
/// target holds a lit lightsaber, until when its target is tired (sim
/// ms), and when the fighter last changed its line before the case began
/// (`Mind::feat_at`, 0 for never). It says how long the case runs and what
/// it must see.
struct Case {
    name: &'static str,
    arms: Arms,
    companion: bool,
    player_picked: bool,
    start_style: Option<Style>,
    target_saber: bool,
    tired_until: u64,
    changed_at: u64,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

const PLAIN: Case = Case {
    name: "",
    arms: Arms::Saber,
    companion: false,
    player_picked: false,
    start_style: None,
    target_saber: false,
    tired_until: 0,
    changed_at: 0,
    seconds: 4,
    wanted: |_| false,
};

/// What a case saw.
#[derive(Debug, Default)]
struct Ran {
    /// Each style the fighter's single blade took, when (sim ms), and
    /// whether a new move of its blade began on that step.
    styles: Vec<(u64, Style, bool)>,
    /// Each style the fighter's mind queued for its single blade, and when.
    queued: Vec<(u64, Style)>,
    /// Each line the fighter's two blades and its pistol took, and when.
    lines: Vec<(u64, Melee, Option<usize>)>,
    /// The line of the fighter's two blades at the end.
    blades: Option<Melee>,
    /// The gun line the fighter chose for its pistol at the end.
    gun: Option<usize>,
    /// The fighter pressed its weapon's other mode, which fires a gun's
    /// feat special.
    fired: bool,
    /// It did so while its target held a lit lightsaber.
    fired_at_saber: bool,
}

impl Ran {
    /// The last style the fighter's single blade took.
    fn last_style(&self) -> Option<Style> {
        self.styles.last().map(|(_, style, _)| *style)
    }

    /// When the single blade first took `wanted` after `after` (sim ms).
    fn style_at(&self, wanted: Style, after: u64) -> Option<u64> {
        (self.styles.iter())
            .find(|(at, style, _)| *style == wanted && *at > after)
            .map(|(at, _, _)| *at)
    }

    /// When the fighter's lines first matched `wanted` after `after`.
    fn line_at(&self, wanted: fn(Melee, Option<usize>) -> bool, after: u64) -> Option<u64> {
        (self.lines.iter())
            .find(|(at, blades, gun)| *at > after && wanted(*blades, *gun))
            .map(|(at, _, _)| *at)
    }
}

/// The time a fighter keeps a changed line (`ai.choices.feats.hold_ms`).
const HOLD: u64 = rules::ai::choices::feats::HOLD_MS;

/// How soon after a style is chosen a fighter that chains its swings takes
/// it, at the start of its next swing (sim ms). A swing of the probe's
/// fighter, with its wind-up and its return, takes less than this.
const NEXT_SWING_MS: u64 = 2_500;

/// The tired case's target is tired until 3 seconds in.
const TIRED_MS: u64 = 3_000;

const CASES: [Case; 11] = [
    Case {
        name: "single_blade_strong_against_a_saber",
        target_saber: true,
        wanted: |ran| ran.last_style() == Some(Style::Strong),
        ..PLAIN
    },
    Case {
        name: "single_blade_fast_against_a_tired_target",
        tired_until: FOREVER,
        wanted: |ran| ran.last_style() == Some(Style::Fast),
        ..PLAIN
    },
    Case {
        name: "single_blade_medium_otherwise",
        start_style: Some(Style::Fast),
        wanted: |ran| ran.last_style() == Some(Style::Medium),
        ..PLAIN
    },
    Case {
        name: "single_blade_style_kept_six_seconds",
        tired_until: TIRED_MS,
        seconds: 12,
        wanted: |ran| {
            // When the mind chose each style, which the blade takes as its
            // next swing begins.
            let chosen = |wanted: Style, after: u64| {
                (ran.queued.iter())
                    .find(|(at, style)| *style == wanted && *at > after)
                    .map(|(at, _)| *at)
            };
            let fast = chosen(Style::Fast, 0);
            let medium = fast.and_then(|fast| chosen(Style::Medium, fast));
            matches!((fast, medium), (Some(fast), Some(medium)) if medium >= fast + HOLD)
        },
        ..PLAIN
    },
    Case {
        name: "single_blade_style_waits_for_the_next_swing",
        tired_until: TIRED_MS,
        seconds: 12,
        wanted: |ran| {
            // The medium style is chosen when the hold after the fast one
            // ends, while the fighter chains its swings. It applies as a
            // new move begins, and soon.
            let fast = ran.style_at(Style::Fast, 0).unwrap_or(0);
            let medium =
                (ran.styles.iter()).find(|(at, style, _)| *style == Style::Medium && *at > fast);
            matches!(medium, Some((at, _, true)) if *at < fast + HOLD + NEXT_SWING_MS)
        },
        ..PLAIN
    },
    Case {
        name: "companion_two_blades_flurry_against_a_tired_target",
        arms: Arms::TwoSabers,
        companion: true,
        tired_until: FOREVER,
        wanted: |ran| ran.blades == Some(Melee::Flurry),
        ..PLAIN
    },
    Case {
        name: "companion_keeps_the_players_line",
        arms: Arms::TwoSabers,
        companion: true,
        player_picked: true,
        tired_until: FOREVER,
        wanted: |ran| ran.blades == Some(Melee::Power),
        ..PLAIN
    },
    Case {
        name: "two_blades_line_kept_six_seconds",
        arms: Arms::TwoSabers,
        tired_until: FOREVER,
        changed_at: 1_000,
        seconds: 9,
        wanted: |ran| {
            let flurry = ran.line_at(|blades, _| blades == Melee::Flurry, 0);
            flurry.is_some_and(|at| at >= 1_000 + HOLD)
        },
        ..PLAIN
    },
    Case {
        name: "gunner_rapid_shot_against_a_close_target",
        arms: Arms::Pistol,
        wanted: |ran| ran.gun == Some(1) && ran.fired,
        ..PLAIN
    },
    Case {
        name: "gunner_line_kept_six_seconds",
        arms: Arms::Pistol,
        changed_at: 1_000,
        seconds: 9,
        wanted: |ran| {
            let rapid = ran.line_at(|_, gun| gun == Some(1), 0);
            rapid.is_some_and(|at| at >= 1_000 + HOLD)
        },
        ..PLAIN
    },
    Case {
        name: "gunner_power_blast_against_a_lightsaber",
        arms: Arms::BlastPistol,
        target_saber: true,
        wanted: |ran| ran.fired_at_saber,
        ..PLAIN
    },
];

/// The fighter's sheet for `case`.
fn fighter_sheet(case: &Case) -> Sheet {
    let (delivery, hands, known): (Delivery, usize, &[u16]) = match case.arms {
        Arms::Saber => (Delivery::Saber(SaberShape::Single), 1, &MELEE),
        Arms::TwoSabers => (Delivery::Saber(SaberShape::Single), 2, &MELEE),
        Arms::Pistol => (Delivery::Blaster(Blaster::Pistol), 1, &GUN),
        Arms::BlastPistol => (Delivery::Blaster(Blaster::Pistol), 1, &GUN[..1]),
    };
    let mut sheet = holding(delivery, hands, known);
    sheet.max_hp = HP;
    // It carries no items, so that no item's wait holds back its gun's
    // special.
    sheet.items = Default::default();
    sheet.faction = if case.companion {
        factions::PLAYER
    } else {
        factions::HOSTILE_1
    };
    if case.player_picked {
        Picks::next(&mut sheet, 0);
    }
    sheet
}

/// The ground set up for `case`, run for its time.
fn run(case: &Case) -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    data.entry.position = crate::determinism::ground_point(PLAYER_AT.0, PLAYER_AT.1, 0.0);
    let creature = |tag: &str, (x, y): (f32, f32), sheet: Sheet| {
        Placement::Own(ObjectData {
            sheet: Some(sheet),
            template: tag.to_string(),
            ..ObjectData::bare(
                ObjectKind::Creature,
                tag,
                crate::determinism::ground_point(x, y, 0.0),
            )
        })
    };
    let at = match case.arms {
        Arms::Pistol | Arms::BlastPistol => CLOSE_AT,
        Arms::Saber | Arms::TwoSabers => FIGHTER_AT,
    };
    data.placements
        .push(creature(FIGHTER, at, fighter_sheet(case)));
    if has_foe(case) {
        let arms = if case.target_saber {
            holding(Delivery::Saber(SaberShape::Single), 1, &[])
        } else {
            Sheet::dev_soldier()
        };
        let faction = if case.companion {
            factions::HOSTILE_1
        } else {
            factions::PLAYER
        };
        let foe = Sheet {
            faction,
            max_hp: HP,
            ..arms
        };
        data.placements.push(creature(FOE, FOE_AT, foe));
    }
    let library = crate::probe::library::<String>("void ActionWait(float fSeconds);", []);
    let nav = super::party_fights_probes::ground_nav(&ground);
    let level = ground.with_module(data).with_scripts(library).with_nav(nav);
    let config = WorldConfig {
        sheet: Sheet {
            max_hp: HP,
            ..Sheet::dev_soldier()
        },
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let player = start.body(Seat(0)).ok_or("no player")?.id;
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let tagged = |tag: &str| {
        (start.entities.iter())
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .map(|entity| entity.id)
            .ok_or(format!("no {tag}"))
    };
    let fighter = tagged(FIGHTER)?;
    let foe = has_foe(case).then(|| tagged(FOE)).transpose()?;
    // The fighter's target: the foe when the case has one, else the player.
    let target = foe.unwrap_or(player);
    if case.companion {
        let mut state = world.script_state().clone();
        state.party.available.insert(
            0,
            Npc {
                template: FIGHTER.to_string(),
                object: Some(fighter.0),
                kept: None,
                template_data: None,
            },
        );
        state.party.members.push(0);
        world.set_script_state(state);
    }
    stage(&mut world, case, (fighter, target), true);
    let mut ran = Ran::default();
    let mut last_move = None;
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        let now = step * u64::from(MSEC);
        if now == case.tired_until {
            stage(&mut world, case, (fighter, target), false);
        }
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, step, &input, MSEC, StepReason::Authority);
        let Some(entity) = snapshot.entities.iter().find(|entity| entity.id == fighter) else {
            continue;
        };
        let Some(fighting) = entity.fighter else {
            continue;
        };
        // A sheet nothing changed stays the placement's.
        let sheet = entity.sheet.unwrap_or_else(|| fighter_sheet(case));
        let saber = fighting.saber;
        if let Some(style) = saber
            .queued
            .filter(|style| ran.queued.last().map(|(_, s)| s) != Some(style))
        {
            ran.queued.push((now, style));
        }
        let moved = last_move.replace(saber.current_move) != Some(saber.current_move);
        if let Some(style) = saber.style.filter(|style| ran.last_style() != Some(*style)) {
            ran.styles.push((now, style, moved));
        }
        let lines = (sheet.picks.blades()[0], sheet.picks.gun(Blaster::Pistol));
        if ran.lines.last().map(|(_, blades, gun)| (*blades, *gun)) != Some(lines) {
            ran.lines.push((now, lines.0, lines.1));
        }
        ran.blades = Some(lines.0);
        ran.gun = lines.1;
        let fired = fighting.last_command.buttons & buttons::ALT_ATTACK != 0;
        let lit = (snapshot.entities.iter())
            .find(|entity| entity.id == target)
            .and_then(|entity| entity.fighter)
            .is_some_and(|target| target.saber.on);
        ran.fired |= fired;
        ran.fired_at_saber |= fired && lit;
    }
    Ok(ran)
}

/// Whether the fighter's target is the creature `FOE` and not the player.
/// A companion fights the foe. So does an enemy whose target holds a
/// lightsaber, because a creature lights its lightsaber in a fight and the
/// player's stays unlit with no keys pressed.
fn has_foe(case: &Case) -> bool {
    case.companion || case.target_saber
}

/// Sets up the fight at the start (`first`), or ends the target's
/// tiredness. At the start the fighter and the foe aim at each other, a
/// companion follows the player, and the fighter takes its first style. The
/// target is tired while its block points are at 0, which nothing refills
/// under the Jedi Academy rules.
fn stage(world: &mut SimWorld, case: &Case, (fighter, target): (SimId, SimId), first: bool) {
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        let fighting = &mut fighting.0;
        if id.0 == target {
            fighting.saber.duel.points = if first && case.tired_until > 0 {
                0
            } else {
                rules::duel::meters::POINTS_MAX
            };
        }
        if !first {
            continue;
        }
        if id.0 == fighter {
            fighting.target = Some(target);
            fighting.combat_until = FOREVER;
            if case.companion {
                fighting.mind.leader = Some(Seat(0));
            }
            if case.start_style.is_some() {
                fighting.saber.style = case.start_style;
            }
            fighting.mind.feat_at = case.changed_at;
        } else if id.0 == target && has_foe(case) {
            fighting.target = Some(fighter);
            fighting.combat_until = FOREVER;
        }
    }
}

pub fn probes() -> Checks {
    CASES
        .iter()
        .map(|case| {
            let result = run(case).and_then(|ran| check((case.wanted)(&ran), format!("{ran:?}")));
            (case.name, result)
        })
        .collect()
}
