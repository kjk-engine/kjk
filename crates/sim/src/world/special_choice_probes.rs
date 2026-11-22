//! The probes of the special saber moves and escapes a fighter starts
//! itself (`kjk sim probes --only "special choices"`; `combat::pace`,
//! `ai.choices.specials`, `ai.choices.escapes`). Each case runs on the
//! proving ground with a script library, as the games' own levels have, and
//! needs no install. The player stands still at one end of the room. A
//! fighter with a vibroblade stands about 7 m away and fights the player, or
//! a companion with a vibroblade fights a hostile creature beside the
//! player. The cases:
//! - a Mandalorian, Skilled by its tag alone, starts a special move within
//!   40 attacks, under the Jedi Academy rules and under the Advanced rules,
//!   and at a player who stands still it starts no kata;
//! - a Boss with a lightsaber starts no special move at a player who circles
//!   it, under both rule sets;
//! - a companion starts a special move and a kata against a stunned foe;
//! - after each special move that hit nothing, a Boss waits the wait after a
//!   miss before its next one. Blades hit nothing on the proving ground
//!   without Jedi Academy's skeleton, so every special move there misses;
//! - a Boss's pace alone, driven think by think, starts special moves at a
//!   still target and none at a target that runs away or around it;
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
use crate::combat::sheet::{Delivery, SaberShape, factions};
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
/// A Boss's tag (`data/duel_bosses.txt`), for cases that need its chance.
const BOSS: &str = "end_bandon";
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
    /// The fighter holds a lightsaber, not a vibroblade.
    saber: bool,
    player: Player,
    rules: SaberRules,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

/// What the player does while the fighter fights it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Player {
    /// It stands still and presses nothing.
    Still,
    /// It faces the fighter and runs sideways around it.
    Circles,
    /// It is stunned for the whole case.
    Stunned,
}

const PLAIN: Case = Case {
    name: "",
    tag: MANDALORIAN,
    companion: false,
    attacks_only: false,
    grenade: false,
    saber: false,
    player: Player::Still,
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
    /// Every attack it began from a ready weapon in the case.
    all_attacks: usize,
    /// Every special move it began in the case, in order.
    moves: Vec<Move>,
    /// Every special move it played to its end: the move, when it began and
    /// ended (sim ms), and whether it hit the target.
    played: Vec<(Move, u64, u64, bool)>,
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

    /// It began no special move in at least `attacks` attacks.
    fn none_in(&self, attacks: usize) -> bool {
        self.moves.is_empty() && self.all_attacks >= attacks
    }

    /// It began a kata, one of the long moves.
    fn kata(&self) -> bool {
        self.moves.iter().any(|current| current.kata())
    }

    /// At least one special move missed, and after each miss the next
    /// special move began no sooner than the wait after a miss
    /// (`ai.choices.specials.miss_wait_ms`).
    fn waits_after_a_miss(&self) -> bool {
        let wait = rules::ai::choices::specials::MISS_WAIT_MS;
        let misses: Vec<u64> = (self.played.iter())
            .filter(|(.., hit)| !hit)
            .map(|(_, _, end, _)| *end)
            .collect();
        !misses.is_empty()
            && misses.iter().all(|end| {
                (self.played.iter())
                    .find(|(_, began, ..)| began > end)
                    .is_none_or(|(_, began, ..)| *began >= end + wait)
            })
    }
}

const CASES: [Case; 12] = [
    Case {
        name: "mandalorian_special_under_ja_rules",
        wanted: |ran| ran.special_soon() && !ran.kata(),
        ..PLAIN
    },
    Case {
        name: "circling_target_gets_no_special",
        tag: BOSS,
        saber: true,
        player: Player::Circles,
        seconds: 120,
        wanted: |ran| ran.none_in(MOST_ATTACKS / 2),
        ..PLAIN
    },
    Case {
        name: "circling_target_gets_no_special_under_advanced_rules",
        tag: BOSS,
        saber: true,
        player: Player::Circles,
        rules: SaberRules::DuelAdvanced,
        seconds: 120,
        wanted: |ran| ran.none_in(MOST_ATTACKS / 4),
        ..PLAIN
    },
    Case {
        name: "stunned_target_gets_a_special_and_a_kata",
        tag: THUG,
        companion: true,
        player: Player::Stunned,
        wanted: |ran| ran.special_soon() && ran.kata(),
        ..PLAIN
    },
    Case {
        name: "miss_makes_the_fighter_wait",
        tag: BOSS,
        saber: true,
        wanted: Ran::waits_after_a_miss,
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

/// The fighter's sheet: a vibroblade or a lightsaber, and no feats.
fn fighter_sheet(case: &Case) -> Sheet {
    let delivery = if case.saber {
        Delivery::Saber(SaberShape::Single)
    } else {
        Delivery::Melee
    };
    let mut sheet = holding(delivery, 1, &[]);
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
    let mut running: Option<(Move, u64)> = None;
    let mut last = start;
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        let now = step * u64::from(MSEC);
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            player_command(case.player, &last, (player, fighter)),
        );
        let snapshot = crate::step(&mut world, step, &input, MSEC, StepReason::Authority);
        let hit_at = (snapshot.entities.iter())
            .find(|entity| entity.id == target)
            .and_then(|entity| entity.fighter)
            .and_then(|target| target.last_attacker)
            .filter(|(by, _)| *by == fighter)
            .map(|(_, at)| at);
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
        if began && let Some((played, since)) = running.take() {
            let hit = hit_at.is_some_and(|at| at >= since);
            ran.played.push((played, since, now, hit));
        }
        if special {
            ran.moves.push(current);
            running = Some((current, now));
        }
        if began && current.start() {
            ran.all_attacks += 1;
        }
        if began && ran.special.is_none() {
            if special {
                ran.special = Some((now, current));
            } else if current.start() {
                ran.attacks += 1;
            }
        }
        last = snapshot;
    }
    Ok(ran)
}

/// The player's command for a step, from where everyone stood after the
/// last one (`last`). Circling faces the fighter.
fn player_command(
    what: Player,
    last: &crate::Snapshot,
    (player, fighter): (SimId, SimId),
) -> Command {
    let place = |id: SimId| {
        (last.entities.iter())
            .find(|entity| entity.id == id)
            .and_then(|entity| entity.player)
    };
    let (Some(ps), Some(foe)) = (place(player), place(fighter)) else {
        return Command::default();
    };
    let (_, yaw) = crate::combat::mind::aim(ps.origin, foe.origin);
    let facing = crate::combat::mind::looking(&ps, 0.0, yaw);
    match what {
        Player::Still | Player::Stunned => Command::default(),
        Player::Circles => Command {
            right: i8::MAX,
            ..facing
        },
    }
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
        } else if id.0 == target {
            if case.companion {
                fighting.target = Some(fighter);
                fighting.combat_until = FOREVER;
            }
            if case.player == Player::Stunned {
                fighting.force.stunned_until = FOREVER;
            }
        }
    }
}

/// How many special moves a Boss with a single blade starts in 300 thinks
/// at a target 60 units away that moves `away` from it and `around` it
/// (units a second), under the Advanced rules (`duel`) or the Jedi Academy
/// rules. The pace alone is driven, with the sight a think would give it
/// (`duelling_probes::duel_sight`). Under the Jedi Academy rules each think
/// shows a new swing of its own, as a chain does.
fn specials_at(duel: bool, away: f32, around: f32) -> usize {
    use crate::combat::pace::{Dice, Keys, Pace, Tier, Told, Way};
    let mut pace = Pace::default();
    let mut started = 0;
    for look in 0..300u64 {
        let now = 1000 + look * crate::combat::pace::THINK_MS;
        let swing = if look % 2 == 0 {
            Move::A_TL2BR
        } else {
            Move::READY
        };
        let sight = crate::combat::pace::Sight {
            tier: Tier::Boss,
            duel,
            style: Some(crate::combat::saber::Style::Medium),
            older: 100 * 100,
            pool: 100,
            foe_away: away,
            foe_around: around,
            saber_move: if duel { 0 } else { swing as u8 },
            ..crate::trace::duelling_probes::duel_sight(60.0)
        };
        let mut dice = Dice::new(now, SimId(1));
        let clear = |_: i8, _: i8| true;
        let (way, reaction) = pace.decide(&sight, now, &mut dice, &clear);
        let forward = match way {
            Way::Advance => i8::MAX,
            Way::Retreat => -i8::MAX,
            Way::Hold => 0,
        };
        let keys = Keys {
            forward,
            ..Keys::default()
        };
        pace.finish(keys, &sight, reaction, now, &mut dice, &clear, 0.0, 1.0);
        if matches!(pace.take_told(), Some(Told::Special(..))) {
            started += 1;
            // The move plays and ends at once, so the next may come.
            pace.special = 0;
        }
    }
    started
}

/// A Boss starts special moves at a target that stands still, and none at a
/// target that runs away from it or around it, under both rule sets.
fn no_special_at_a_leaving_target() -> Result<String, String> {
    let run = rules::movement::ground::BASE_SPEED;
    let got: Vec<(bool, [usize; 3])> = [true, false]
        .into_iter()
        .map(|duel| {
            let counts = [
                specials_at(duel, 0.0, 0.0),
                specials_at(duel, run, 0.0),
                specials_at(duel, 0.0, run),
            ];
            (duel, counts)
        })
        .collect();
    let right = got
        .iter()
        .all(|(_, [still, away, around])| *still > 0 && *away == 0 && *around == 0);
    check(
        right,
        format!(
            "special moves at a still, a leaving and a circling target, (advanced, counts): {got:?}"
        ),
    )
}

pub fn probes() -> Checks {
    let mut checks: Checks = CASES
        .iter()
        .map(|case| {
            let result = run(case).and_then(|ran| check((case.wanted)(&ran), format!("{ran:?}")));
            (case.name, result)
        })
        .collect();
    checks.push((
        "no_special_at_a_leaving_target",
        no_special_at_a_leaving_target(),
    ));
    checks
}
