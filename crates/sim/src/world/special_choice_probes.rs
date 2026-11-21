//! The probes of the special saber moves and escapes a fighter starts
//! itself (`kjk sim probes --only "special choices"`; `combat::pace`,
//! `ai.choices.specials`, `ai.choices.escapes`). Each case runs on the
//! proving ground with a script library, as the games' own levels have, and
//! needs no install. The player stands still at one end of the room. A
//! fighter with a vibroblade stands about 7 m away and fights the player, or
//! a companion with a vibroblade fights a hostile creature beside the
//! player. The cases:
//! - a Mandalorian, Skilled by its tag alone, starts a special move within
//!   40 attacks, under the Jedi Academy rules and under the Advanced rules;
//! - a companion, Skilled because it is a companion, starts one within 40
//!   attacks;
//! - a Skilled fighter rolls or cartwheels away from a grenade that rests
//!   beside it within 3 seconds, under both rule sets;
//! - an ordinary enemy with the same grenade beside it starts no special
//!   move and no escape in 30 seconds;
//! - a Mandalorian that its scripts set to attack with its weapon only starts
//!   no special move and no escape in 30 seconds.

use super::{Bolt, Fighting, Id, SimWorld};
use crate::combat::Missile;
use crate::combat::Sheet;
use crate::combat::grenades::Lob;
use crate::combat::saber::{Move, SaberRules};
use crate::combat::sheet::{Delivery, factions};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::party::Npc;
use crate::snapshot::SimId;
use crate::world::picks_probes::holding;
use crate::{StepReason, WorldConfig};

/// A step's milliseconds.
const MSEC: u32 = 50;
/// The hit points of everyone, so that nobody falls during a case.
const HP: u16 = 20_000;
/// How long the fights last (sim ms). This is longer than any case.
const FOREVER: u64 = 10_000_000;

/// The fighter's tag when it is a Mandalorian (`data/duel_skilled.txt`), and
/// when it is an ordinary enemy.
const MANDALORIAN: &str = "kas25_mandalor01";
const THUG: &str = "probe_thug";
const FOE: &str = "probe_foe";

/// Where everyone stands (metres in the room's frame).
const PLAYER_AT: (f32, f32) = (-2.0, -4.0);
const FIGHTER_AT: (f32, f32) = (-2.0, 3.0);
const FOE_AT: (f32, f32) = (-2.0, -1.0);
/// How far to the fighter's side the grenade rests (metres).
const GRENADE_BESIDE: f32 = 1.5;
/// An id no creature of the ground takes, for the grenade.
const GRENADE_ID: u32 = 900_000;

/// The most attacks before a Skilled fighter's first special move: four
/// times the attacks of its chance (`ai.choices.specials.skilled_one_in`).
/// The sim is deterministic, so a case that meets it always does.
const MOST_ATTACKS: usize = 4 * rules::ai::choices::specials::SKILLED_ONE_IN as usize;

/// A case: the fighter's tag, whether it is a companion, whether its scripts
/// set it to attack only, whether a grenade rests beside it, the rules, how
/// long it runs (seconds) and what it must see.
struct Case {
    name: &'static str,
    tag: &'static str,
    companion: bool,
    attacks_only: bool,
    grenade: bool,
    rules: SaberRules,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

const PLAIN: Case = Case {
    name: "",
    tag: MANDALORIAN,
    companion: false,
    attacks_only: false,
    grenade: false,
    rules: SaberRules::Ja,
    seconds: 180,
    wanted: |_| false,
};

/// What a case saw.
#[derive(Debug, Default)]
struct Ran {
    /// The attacks the fighter began from a ready weapon (a swing's start
    /// move) before its first special move. The swings it chains onto an
    /// attack are not counted.
    attacks: usize,
    /// When it began its first special move (sim ms), and which.
    special: Option<(u64, Move)>,
    /// When it first rolled or cartwheeled (sim ms).
    escaped: Option<u64>,
    /// Every special move it began in the case, in order.
    moves: Vec<Move>,
}

impl Ran {
    fn special_soon(&self) -> bool {
        self.special.is_some() && self.attacks <= MOST_ATTACKS
    }

    fn escaped_within(&self, ms: u64) -> bool {
        self.escaped.is_some_and(|at| at <= ms)
    }

    fn nothing(&self) -> bool {
        self.special.is_none() && self.escaped.is_none()
    }
}

const CASES: [Case; 8] = [
    Case {
        name: "mandalorian_special_under_ja_rules",
        wanted: Ran::special_soon,
        ..PLAIN
    },
    Case {
        name: "mandalorian_special_under_advanced_rules",
        rules: SaberRules::DuelAdvanced,
        wanted: Ran::special_soon,
        ..PLAIN
    },
    Case {
        name: "companion_special",
        tag: THUG,
        companion: true,
        wanted: Ran::special_soon,
        ..PLAIN
    },
    Case {
        name: "skilled_escapes_a_grenade_under_ja_rules",
        grenade: true,
        seconds: 3,
        wanted: |ran| ran.escaped_within(3_000),
        ..PLAIN
    },
    Case {
        name: "skilled_escapes_a_grenade_under_advanced_rules",
        grenade: true,
        rules: SaberRules::DuelAdvanced,
        seconds: 3,
        wanted: |ran| ran.escaped_within(3_000),
        ..PLAIN
    },
    Case {
        name: "ordinary_enemy_does_neither",
        tag: THUG,
        grenade: true,
        seconds: 30,
        wanted: Ran::nothing,
        ..PLAIN
    },
    Case {
        name: "ordinary_enemy_does_neither_under_advanced_rules",
        tag: THUG,
        grenade: true,
        rules: SaberRules::DuelAdvanced,
        seconds: 30,
        wanted: Ran::nothing,
        ..PLAIN
    },
    Case {
        name: "attacks_only_does_neither",
        attacks_only: true,
        grenade: true,
        seconds: 30,
        wanted: Ran::nothing,
        ..PLAIN
    },
];

/// The fighter's sheet: a vibroblade and no feats.
fn fighter_sheet(case: &Case) -> Sheet {
    let mut sheet = holding(Delivery::Melee, 1, &[]);
    sheet.max_hp = HP;
    sheet.items = Default::default();
    sheet.faction = if case.companion {
        factions::PLAYER
    } else {
        factions::HOSTILE_1
    };
    sheet
}

/// Whether `current` is a cartwheel.
fn cartwheel(current: Move) -> bool {
    matches!(
        current,
        Move::JUMPATTACK_ARIAL_LEFT | Move::JUMPATTACK_ARIAL_RIGHT
    )
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
    data.placements
        .push(creature(case.tag, FIGHTER_AT, fighter_sheet(case)));
    if case.companion {
        // It carries no items, so that no shield or stealth of its own
        // changes the fight.
        let foe = Sheet {
            faction: factions::HOSTILE_1,
            max_hp: HP,
            items: Default::default(),
            ..holding(Delivery::Melee, 1, &[])
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
    world.set_saber_rules(case.rules);
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
    let fighter = tagged(case.tag)?;
    let target = if case.companion { tagged(FOE)? } else { player };
    let mut state = world.script_state().clone();
    if case.companion {
        state.party.available.insert(
            0,
            Npc {
                template: case.tag.to_string(),
                object: Some(fighter.0),
                kept: None,
                template_data: None,
            },
        );
        state.party.members.push(0);
    }
    if case.attacks_only {
        let style = crate::script::party::NPC_AISTYLE_MELEE_ATTACK;
        state.party.ai_styles.insert(fighter.0, style);
    }
    world.set_script_state(state);
    stage(&mut world, case, (fighter, target));
    if case.grenade {
        let at = (start.entities.iter())
            .find(|entity| entity.id == fighter)
            .and_then(|entity| entity.player)
            .map(|ps| ps.origin)
            .ok_or("no fighter's place")?;
        let beside = [at[0] + GRENADE_BESIDE / collision::UNIT, at[1], at[2]];
        rest_grenade(&mut world, player, beside);
    }
    let mut ran = Ran::default();
    let mut last_move = None;
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        let now = step * u64::from(MSEC);
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, step, &input, MSEC, StepReason::Authority);
        let Some(entity) = snapshot.entities.iter().find(|entity| entity.id == fighter) else {
            continue;
        };
        let Some(fighting) = entity.fighter else {
            continue;
        };
        let current = fighting.saber.current_move;
        let before = last_move.replace(current);
        let began = before != Some(current);
        let rolling = entity.player.is_some_and(|ps| {
            crate::motion::anim::is_roll(ps.legs.anim)
                || crate::motion::anim::is_cartwheel(ps.legs.anim)
        });
        if rolling && ran.escaped.is_none() {
            ran.escaped = Some(now);
        }
        let special = began && current.special() && !cartwheel(current);
        if special {
            ran.moves.push(current);
        }
        if began && ran.special.is_none() {
            if special {
                ran.special = Some((now, current));
            } else if current.start() {
                ran.attacks += 1;
            }
        }
    }
    Ok(ran)
}

/// Puts a grenade that rests at `at` and does not burst within any case.
fn rest_grenade(world: &mut SimWorld, thrower: SimId, at: [f32; 3]) {
    world.world.spawn((
        Id(SimId(GRENADE_ID)),
        Bolt(Missile {
            owner: thrower,
            weapon: Sheet::dev_soldier().main_weapon(),
            cadence_ms: 0,
            share: 1000,
            base: at,
            from_ms: 0,
            velocity: [0.0; 3],
            half_size: crate::combat::grenades::HALF_SIZE,
            blockable: false,
            until_ms: FOREVER,
            lob: Some(Lob {
                row: 0,
                resting: true,
            }),
            held: None,
            scripted: false,
            power: false,
        }),
    ));
}

/// The fighter and its target aim at each other, the fighter's weapon is
/// drawn, and a companion follows the player.
fn stage(world: &mut SimWorld, case: &Case, (fighter, target): (SimId, SimId)) {
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        let fighting = &mut fighting.0;
        if id.0 == fighter {
            fighting.target = Some(target);
            fighting.combat_until = FOREVER;
            if case.companion {
                fighting.mind.leader = Some(Seat(0));
            }
            // Its weapon is drawn at once, as in a fight under way.
            fighting.saber.on = true;
        } else if id.0 == target && case.companion {
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
