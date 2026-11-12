//! Passive feats and feat prerequisites (`combat::passive`, `levels`):
//! their rules on sheets and classes made here, and in a world on the
//! proving ground, where the host dresses a companion nobody plays (a
//! level 3 soldier, CON 14, 30 HP, Melee Weapon Proficiency only):
//!
//! 0. Heavy armour and a level 1 implant are refused it, with the game's
//!    line; a blade is worn.
//! 1. A script grants it the three armour proficiencies and Toughness
//!    (`GrantFeat`): 3 HP more, one a level.
//! 2. The armour is worn now.
//! 3. The party has the XP for level 4, whose class grants Master
//!    Toughness, and the host asks it (the game's picks): the die, CON, 2
//!    for the two Toughness feats and 3 for
//!    Master Toughness's earlier levels. Hurt to 10 HP before it, it is
//!    whole at its new most after (both games' level-up).
//!
//! For the approved test `feats`.

use fmt_odyssey::ncs::{Const, Op};

use super::{Fighting, Id, SimWorld};
use crate::action::Action;
use crate::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use crate::combat::passive::{self, Game};
use crate::combat::sheet::{Ability, Rows, Sheet, Weapon, damage_types, feats, skills, type_rows};
use crate::combat::{Location, grenades};
use crate::input::Seat;
use crate::inventory::{Catalog, ItemFacts, PARTY, slots};
use crate::levels::{Career, Choices, Class, Classes, Needs, Refusal, SKILLS, Unpickable};
use crate::module::{ObjectKind, Placement};
use crate::notice::Notice;
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::snapshot::SimId;
use crate::{Snapshot, StepReason, TickInput, WorldConfig};

const ALLY: &str = "ally";
/// The companion's class and level as placed, and its CON and HP.
const START: (u8, u8) = (0, 3);
const CON: u8 = 14;
const HP: u16 = 30;
/// The XP for level 4.
const XP: i32 = 6000;
/// The granting script.
const GRANT: &str = "probe_grant";
const GRANTED: [u16; 4] = [4, 5, 6, 84];
const MASTER_TOUGHNESS: u16 = 124;
/// The companion's hit points (whole) as it takes level 4.
const HURT: u16 = 10;

/// A sheet with `feats`, the companion's numbers.
fn soldier(known: &[u16]) -> Sheet {
    let mut abilities = [12; 6];
    abilities[Ability::Constitution as usize] = CON;
    Sheet {
        abilities,
        level: START.1,
        bab: START.1,
        max_hp: HP,
        feats: known
            .iter()
            .fold(Rows::default(), |rows, feat| rows.with(*feat)),
        skills: [0; SKILLS],
        ..Sheet::dev_soldier()
    }
}

/// An item worn in `slot` that needs `needs`.
fn needing(template: &str, slot: u8, needs: &[u16]) -> ItemFacts {
    ItemFacts {
        slots: 1 << slot,
        needs: needs.to_vec(),
        ..super::containers::plain(template)
    }
}

fn catalog() -> Catalog {
    Catalog::new([
        needing("plate", slots::BODY, &[4, 5, 6]),
        needing("implant", slots::IMPLANT, &[14]),
        needing("blade", slots::RIGHT_WEAPON, &[feats::PROFICIENCY_MELEE]),
    ])
}

/// A class of twenty levels: a d10, a feat to pick a level from
/// `picks`, `granted` at its levels.
fn class(picks: &[u16], granted: &[(u8, u16)]) -> Class {
    Class {
        hit_die: 10,
        skill_points: 2,
        bab: (1..=20).collect(),
        saves: vec![[2, 0, 0]; 20],
        defence: vec![0; 20],
        feat_picks: vec![u8::from(!picks.is_empty()); 20],
        power_picks: vec![0; 20],
        granted: granted.to_vec(),
        feats: picks.to_vec(),
        class_skills: [true; SKILLS],
        ..Class::default()
    }
}

fn classes(classes: Vec<Class>) -> Classes {
    Classes {
        classes,
        levels: vec![0, 1000, 3000, XP, 10_000],
        ..Classes::default()
    }
}

/// `probe_grant`: `GrantFeat(f, OBJECT_SELF)` for each granted feat.
fn grant_script() -> crate::script::Library {
    const CATALOG: &str = "// 0\nvoid GrantFeat(int nFeat, object oCreature);\n";
    let mut asm = script_ncs::probes::Asm::default();
    for feat in GRANTED {
        asm.op(Op::Const(Const::Object(0)))
            .op(Op::Const(Const::Int(i32::from(feat))))
            .op(Op::Action {
                routine: 0,
                args: 2,
            });
    }
    asm.op(Op::Retn);
    crate::probe::library(CATALOG, [(GRANT, asm.build())])
}

/// Each step's snapshot and the notices told seat 0, by step.
struct Ran {
    ally: u32,
    snapshots: Vec<Snapshot>,
    notices: Vec<(usize, Notice)>,
    worn: Vec<Vec<(Option<u8>, String)>>,
}

fn scenario() -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    data.placements
        .push(Placement::Own(crate::module::ObjectData {
            sheet: Some(soldier(&[feats::PROFICIENCY_MELEE])),
            classes: vec![START],
            template: ALLY.to_string(),
            ..crate::module::ObjectData::bare(
                ObjectKind::Creature,
                ALLY,
                [entry[0] + 1.5, entry[1], entry[2]],
            )
        }));
    let library = grant_script()
        .with_items(catalog())
        .with_classes(classes(vec![class(&[], &[(4, MASTER_TOUGHNESS)])]));
    let mut world = SimWorld::new(
        WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let ally = start
        .entities
        .iter()
        .find(|entity| {
            objects
                .of_entity(entity)
                .is_some_and(|data| data.tag == ALLY)
        })
        .map(|entity| entity.id.0)
        .ok_or("no ally")?;
    let mut state = world.script_state().clone();
    state.party.available.insert(
        0,
        crate::script::party::Npc {
            template: ALLY.to_string(),
            object: Some(ally),
            kept: None,
            template_data: None,
        },
    );
    state.party.members.push(0);
    let items = catalog();
    let mut made = |template: &str| {
        state
            .make_item(&items, template, PARTY, 1, None)
            .map(|id| id.0)
            .ok_or(format!("no {template}"))
    };
    let (plate, implant, blade) = (made("plate")?, made("implant")?, made("blade")?);
    world.set_script_state(state);
    // It follows seat 0, as `AddPartyMember` leaves it.
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        if id.0.0 == ally {
            fighting.0.mind.leader = Some(Seat(0));
        }
    }

    let dress = |item, slot| Action::EquipOn {
        who: ally,
        item,
        slot,
    };
    let steps: [Vec<Action>; 4] = [
        vec![
            dress(plate, slots::BODY),
            dress(implant, slots::IMPLANT),
            dress(blade, slots::RIGHT_WEAPON),
        ],
        Vec::new(),
        vec![dress(plate, slots::BODY)],
        vec![Action::LevelUpOn {
            who: ally,
            choices: Choices {
                class: START.0,
                ..Choices::default()
            },
        }],
    ];
    let (mut snapshots, mut notices, mut worn) = (Vec::new(), Vec::new(), Vec::new());
    for (at, actions) in steps.into_iter().enumerate() {
        let mut state = world.script_state().clone();
        match at {
            1 => state.pending.push(crate::script::Pending::new(
                GRANT,
                script_ncs::vm::ObjectId(ally),
            )),
            3 => {
                state.xp.insert(PARTY, XP);
            }
            _ => {}
        }
        world.set_script_state(state);
        if at == 3 {
            hurt(&mut world, ally);
        }
        let mut input = TickInput::default();
        input.insert(Seat(0), crate::Command::default());
        for action in actions {
            input.act(Seat(0), action);
        }
        snapshots.push(crate::step(
            &mut world,
            at as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        let state = world.script_state();
        worn.push(
            state
                .items
                .held_by(ally)
                .map(|(_, item)| (item.slot, item.template.clone()))
                .collect(),
        );
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .filter(|(seat, _)| *seat == Seat(0))
                .map(|(_, notice)| (at, notice)),
        );
    }
    Ok(Ran {
        ally,
        snapshots,
        notices,
        worn,
    })
}

/// The companion down to `HURT` hit points (whole).
fn hurt(world: &mut SimWorld, ally: u32) {
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        if id.0.0 == ally {
            fighting.0.hp = i32::from(HURT) * 100;
        }
    }
}

/// The scenario's checks, the rules' checks, and that two runs end alike.
pub fn probes() -> Checks {
    let mut checks = rules();
    let ran = match scenario() {
        Ok(ran) => ran,
        Err(why) => {
            checks.push(("feats_scenario", Err(why)));
            return checks;
        }
    };
    checks.extend(world_cases(&ran));
    let again = scenario().map(|again| again.snapshots.last().map(Snapshot::hash));
    let last = ran.snapshots.last().map(Snapshot::hash);
    checks.push((
        "same_every_run",
        check(
            again.as_ref().is_ok_and(|hash| *hash == last),
            format!(
                "{:?} then {:?}",
                last.map(|hash| hash.short()),
                again.map(|hash| hash.map(|hash| hash.short()))
            ),
        ),
    ));
    checks
}

fn world_cases(ran: &Ran) -> Checks {
    let sheet = |step: usize| {
        ran.snapshots[step]
            .get(SimId(ran.ally))
            .and_then(|entity| entity.sheet)
    };
    let refused = Notice::Line(passive::NOT_PROFICIENT);
    let told = |step: usize| {
        ran.notices
            .iter()
            .filter(|(at, notice)| *at == step && *notice == refused)
            .count()
    };
    let wears = |step: usize, template: &str| {
        ran.worn[step]
            .iter()
            .any(|(slot, worn)| slot.is_some() && worn == template)
    };
    let hp = |step: usize| sheet(step).map(|sheet| sheet.max_hp);
    let hp_now = |step: usize| {
        ran.snapshots[step]
            .get(SimId(ran.ally))
            .and_then(|entity| entity.fighter)
            .map(|fighter| fighter.hp)
    };
    let granted = sheet(1).is_some_and(|sheet| GRANTED.iter().all(|feat| sheet.feats.has(*feat)));
    vec![
        (
            "unproficient_equip_refused",
            check(
                told(0) == 2 && !wears(0, "plate") && !wears(0, "implant"),
                format!("told {} times; worn {:?}", told(0), ran.worn[0]),
            ),
        ),
        (
            "proficient_equip_worn",
            check(wears(0, "blade"), format!("worn {:?}", ran.worn[0])),
        ),
        (
            "grant_feat_toughness",
            check(
                granted && hp(1) == Some(HP + u16::from(START.1)),
                format!("granted {granted}, HP {HP} → {:?}", hp(1)),
            ),
        ),
        (
            "granted_proficiency_equips",
            check(
                wears(2, "plate") && told(2) == 0,
                format!("worn {:?}", ran.worn[2]),
            ),
        ),
        (
            "level_up_heals",
            check(
                sheet(3).is_some_and(|sheet| hp_now(3) == Some(i32::from(sheet.max_hp) * 100)),
                format!(
                    "hurt to {HURT} HP, after the level {:?} hundredths (most {:?})",
                    hp_now(3),
                    hp(3)
                ),
            ),
        ),
        (
            "level_up_toughness",
            check(
                sheet(3).is_some_and(|sheet| {
                    sheet.level == 4
                        && sheet.feats.has(MASTER_TOUGHNESS)
                        && Some(sheet.max_hp) == hp(2).map(|hp| hp + 10 + 2 + 2 + 3)
                }),
                format!(
                    "level {:?}, HP {:?} → {:?}",
                    sheet(3).map(|sheet| sheet.level),
                    hp(2),
                    hp(3)
                ),
            ),
        ),
    ]
}

/// A weapon of base item `row`.
fn weapon(row: u16, proficiency: u16) -> Weapon {
    Weapon::plain(
        row,
        1,
        8,
        (1, 2),
        damage_types::SLASHING,
        [proficiency, u16::MAX, u16::MAX],
    )
}

fn with(known: &[u16], hands: [Option<Weapon>; 2]) -> Sheet {
    Sheet {
        weapons: hands,
        ..soldier(known)
    }
}

/// The rules on sheets and classes made here.
fn rules() -> Checks {
    let (blade, sword, pistol, staff, rifle) = (
        weapon(5, feats::PROFICIENCY_MELEE),
        weapon(2, feats::PROFICIENCY_MELEE),
        weapon(12, feats::PROFICIENCY_BLASTER),
        weapon(9, feats::PROFICIENCY_LIGHTSABER),
        weapon(77, feats::PROFICIENCY_BLASTER_RIFLE),
    );
    let two = &feats::TWO_WEAPON_FIGHTING;
    let hands = |known: &[u16], hands| {
        let sheet = with(known, hands);
        [0, 1].map(|hand| passive::hand_attack(&sheet, hand))
    };
    let pair = [Some(sword), Some(sword)];
    let tiers: Vec<[i32; 2]> = (0..=3).map(|tier| hands(&two[..tier], pair)).collect();
    let balanced = hands(&[], [Some(sword), Some(blade)]);
    let double = hands(&two[..1], [Some(staff), None]);
    let duel = |hands: [Option<Weapon>; 2]| {
        let sheet = with(&feats::DUELING, hands);
        (
            passive::hand_attack(&sheet, 0),
            sheet.defence() - with(&[], hands).defence(),
        )
    };
    let bonus = |sheet: &Sheet, weapon: &Weapon| damage::attack_bonus(sheet, weapon);
    let plain = with(&[], [Some(blade), None]);
    let trained = with(&[feats::PROFICIENCY_MELEE], [Some(blade), None]);
    let all = with(&[feats::PROFICIENCY_ALL], [Some(blade), None]);
    let saves = |known: &[u16]| [0, 1, 2].map(|index| with(known, [None; 2]).save(index));
    let skilled = {
        let mut sheet = with(&[12, 119, 75, 7, 117, 118, 27, 10], [None; 2]);
        sheet.skills[skills::SECURITY] = 4;
        sheet.skills[skills::STEALTH] = 1;
        sheet
    };
    let reflex = |known: &[u16]| grenades::reflex(&with(known, [None; 2]), 20, 15);
    vec![
        (
            "two_weapon_penalties",
            check(
                tiers == [[-6, -10], [-6, -6], [-4, -4], [-2, -2]],
                format!("main/off by tier {tiers:?}"),
            ),
        ),
        (
            "balanced_off_hand",
            check(
                balanced == [-4, -10] && double == [-6, -6],
                format!("a small blade in the left {balanced:?}; a staff, the feat {double:?}"),
            ),
        ),
        (
            "dueling_attack_and_defence",
            check(
                duel([Some(pistol), None]) == (3, 3)
                    && duel([Some(rifle), None]) == (0, 0)
                    && duel([Some(sword), Some(blade)]).1 == 0,
                format!(
                    "Master Dueling: a pistol {:?}, a rifle {:?}, two blades {:?}",
                    duel([Some(pistol), None]),
                    duel([Some(rifle), None]),
                    duel([Some(sword), Some(blade)])
                ),
            ),
        ),
        (
            "proficiency_to_hit",
            check(
                bonus(&trained, &blade) - bonus(&plain, &blade) == 4
                    && bonus(&all, &blade) == bonus(&trained, &blade),
                format!(
                    "untrained {}, trained {}, Proficiency All {}",
                    bonus(&plain, &blade),
                    bonus(&trained, &blade),
                    bonus(&all, &blade)
                ),
            ),
        ),
        (
            "may_equip",
            check(
                !passive::may_equip(&trained.feats, &[4, 5, 6])
                    && passive::may_equip(&all.feats, &[4, 5, 6])
                    && !passive::may_equip(&all.feats, &[14]),
                "heavy armour: refused untrained, worn with Proficiency All; an implant \
                 still needs its feat"
                    .into(),
            ),
        ),
        (
            "conditioning_saves",
            check(
                saves(&feats::CONDITIONING[..2])
                    .iter()
                    .zip(saves(&[]))
                    .all(|(with, without)| with - without == 2),
                format!(
                    "{:?} with Improved Conditioning, {:?} without",
                    saves(&feats::CONDITIONING[..2]),
                    saves(&[])
                ),
            ),
        ),
        (
            "uncanny_dodge_grenades",
            check(
                !reflex(&[]) && reflex(&feats::UNCANNY_DODGE),
                "d20 15 against DC 20: failed, made with Uncanny Dodge 2".into(),
            ),
        ),
        (
            "skill_feats",
            check(
                skilled.skill(skills::SECURITY) == 4 + 2 + 2
                    && skilled.skill(skills::STEALTH) == 1 + 3 + 2
                    && skilled.skill(skills::PERSUADE) == 0,
                format!(
                    "Security {}, Stealth {}, Persuade without ranks {}",
                    skilled.skill(skills::SECURITY),
                    skilled.skill(skills::STEALTH),
                    skilled.skill(skills::PERSUADE)
                ),
            ),
        ),
        reduction(),
        levels(),
        most_force_points(),
        chart(),
        power_chart(),
        same_level_picks(),
        ranks_apart_from_bonuses(),
    ]
}

/// Skill ranks kept apart from what items and feats add, as both games keep
/// a skill's stored rank apart from its total. A level 3 soldier has 5
/// Repair ranks of its own, Gear Head, Intelligence 12 and a belt that gives
/// +4 Repair. Its total, as `GetSkillRank` reads it in both games, is 5 + 4 +
/// Gear Head's + 1. Its level 4, with 4 skill points banked from earlier
/// levels, still buys Repair ranks up to the cap of 7 (the level +
/// `rules::progression::levels::RANK_CAP_OVER`), because the cap counts its
/// own ranks alone. One rank past the cap is refused, and the level taken
/// keeps the belt's +4 out of the ranks.
fn ranks_apart_from_bonuses() -> (&'static str, Result<String, String>) {
    let classes = classes(vec![class(&[], &[])]);
    let gear_head = feats::SKILL_LINES[2].0[0];
    let mut belt = super::containers::plain("belt");
    belt.slots = 1 << slots::BELT;
    belt.gear.skills[skills::REPAIR] = 4;
    let mut own = soldier(&[gear_head]);
    own.skills[skills::REPAIR] = 5;
    let worn = crate::inventory::dress(own, [(slots::BELT, &belt)]);
    let now = crate::combat::effects::sheet_now(worn, &crate::combat::Fighter::of(&worn), 0);
    let total = |tsl| crate::stealth::rank(None, &worn, &now, skills::REPAIR, tsl);
    let expected_total = 5 + 4 + rules::feats::skills::LINE_PER_TIER[2] + 1;
    let career = vec![START];
    let buying = |ranks: u8| {
        let mut skills = [0; SKILLS];
        skills[skills::REPAIR] = ranks;
        let choices = Choices {
            class: 0,
            ability: Some(Ability::Strength as u8),
            skills,
            ..Choices::default()
        };
        classes.taken(&worn, &career, &choices, 4)
    };
    let cap = START.1 + 1 + rules::progression::levels::RANK_CAP_OVER;
    let to_cap = buying(2).map(|(after, ..)| {
        (
            after.skills[skills::REPAIR],
            after.gear.skills[skills::REPAIR],
        )
    });
    let past = buying(3).err();
    let most = Refusal::Ranks {
        skill: skills::REPAIR as u8,
        most: cap,
    };
    (
        "ranks_apart_from_bonuses",
        check(
            [total(false), total(true)] == [expected_total; 2]
                && to_cap == Ok((cap, 4))
                && past == Some(most),
            format!(
                "Repair total {} (K1) and {} (TSL), two ranks bought: {to_cap:?} (ranks, the \
                 belt's), three: {past:?}",
                total(false),
                total(true)
            ),
        ),
    )
}

/// A Force user's most FP as both games work it out (`Sheet::force_points`),
/// on Darth Malak's numbers (level 20, a template's 35, WIS 15, CHA 20):
/// K1 adds the Wisdom and Charisma modifiers a level (175), TSL Wisdom's
/// alone with its bonus (35 + 2 × 20 + 10); a Wisdom item counts at once; a
/// low score never takes it under the level; anyone else has none.
fn most_force_points() -> (&'static str, Result<String, String>) {
    let jedi = |wisdom: u8, charisma: u8, tsl: bool| {
        let mut sheet = Sheet::dev_soldier();
        sheet.level = 20;
        sheet.max_fp = 35;
        sheet.abilities[Ability::Wisdom as usize] = wisdom;
        sheet.abilities[Ability::Charisma as usize] = charisma;
        if tsl {
            sheet.flags |= crate::combat::sheet::flags::TSL;
            sheet.bonus_fp = 10;
        }
        sheet.force_points()
    };
    let none = Sheet {
        level: 20,
        ..Sheet::dev_soldier()
    };
    let seen = [
        jedi(15, 20, false),
        jedi(17, 20, false),
        jedi(15, 20, true),
        jedi(3, 3, false),
        none.force_points(),
    ];
    (
        "most_force_points",
        check(
            seen == [175, 195, 85, 20, 0],
            format!(
                "K1 {}, WIS 17 {}, TSL {}, low scores {}, a soldier {}",
                seen[0], seen[1], seen[2], seen[3], seen[4]
            ),
        ),
    )
}

/// Improved and Wookiee Toughness on a KOTOR attack's whole points, and
/// through the pipeline.
fn reduction() -> (&'static str, Result<String, String>) {
    let tough = |known: &[u16], game, row, points| {
        let feats = known
            .iter()
            .fold(Rows::<4>::default(), |rows, feat| rows.with(*feat));
        passive::toughness(&feats, game, Some(usize::from(row)), points)
    };
    let improved = [feats::IMPROVED_TOUGHNESS];
    let numbers = [
        tough(&improved, Game::K1, type_rows::SLASHING, 10),
        tough(&improved, Game::K1, type_rows::ENERGY, 10),
        tough(&improved, Game::Tsl, type_rows::ENERGY, 40),
        tough(
            &feats::WOOKIEE_TOUGHNESS[..1],
            Game::K1,
            type_rows::ENERGY,
            10,
        ),
        tough(
            &feats::WOOKIEE_TOUGHNESS[..2],
            Game::Tsl,
            type_rows::ENERGY,
            45,
        ),
        tough(&improved, Game::K1, type_rows::SLASHING, 1),
    ];
    let blow = Blow {
        attacker: None,
        relation: Relation::Hostile,
        source: Source::Weapon {
            weapon: weapon(5, feats::PROFICIENCY_MELEE),
            cadence_ms: 3000,
            share: 1000,
            area: false,
        },
        location: Location::Chest,
        direction: None,
        sneak: 0,
    };
    let dealt = |known: &[u16]| {
        (0..50)
            .map(|seed| {
                let target = with(known, [None; 2]);
                let assessed = damage::assess(
                    &blow,
                    None,
                    &target,
                    FriendlyFire::Off,
                    Game::K1,
                    &mut SimRng::new(seed, 7),
                );
                i64::from(assessed.damage)
            })
            .sum::<i64>()
    };
    let (bare, hardy) = (dealt(&[]), dealt(&improved));
    (
        "damage_reduction",
        check(
            numbers == [2, 0, 2, 2, 7, 1] && hardy < bare,
            format!("{numbers:?}; 50 blades' hundredths {bare} → {hardy} with Improved Toughness"),
        ),
    )
}

/// Levels taken: Toughness picked (its earlier levels too), Wookiee
/// Toughness's extra HP, Force Sensitive granted with a new class, and an
/// ability minimum.
/// KOTOR's feat chart (`Classes::chart`) on a class made here: Flurry's
/// chain (11, 91 needing it, 53 needing both) a row; a granted feat known
/// leads its own (28, its follower 17); one granted and not known (5)
/// and one known the class may not pick (113) lead none; a follower the class
/// neither picks nor grants (83) is left out.
fn chart() -> (&'static str, Result<String, String>) {
    let mut classes = classes(vec![class(&[11, 53, 91, 17], &[(1, 28), (1, 5)])]);
    for (feat, needs) in [(91, [11, 0]), (53, [11, 91]), (17, [28, 0]), (83, [28, 17])] {
        let needs = needs.map(|need| (need != 0).then_some(need));
        classes.needs.insert(
            feat,
            Needs {
                feats: needs,
                ..Needs::default()
            },
        );
    }
    let class = &classes.classes[0];
    let made = classes.chart(class, &[28, 113]);
    let expected = vec![[Some(11), Some(91), Some(53)], [Some(28), Some(17), None]];
    // A first level knows what the class grants at 1, its own chart's too.
    let known = classes
        .picks(&soldier(&[]), &Vec::new(), 0)
        .map(|picks| picks.known);
    (
        "feat_chart",
        check(
            made == expected && known == Some(vec![5, 28]),
            format!("{made:?}, known {known:?}"),
        ),
    )
}

/// KOTOR's power chart (`Classes::power_chart`) on `spells.2da`'s rows:
/// Push, Whirlwind (needing Push) and Wave (needing both) a row, Wound,
/// Choke and Kill another; TSL's Crush Opposition I-III a row and IV-VI,
/// whose IV needs III, one of their own. Wave asks for Whirlwind as well
/// as Push (`why_not_power`).
fn power_chart() -> (&'static str, Result<String, String>) {
    let mut classes = classes(vec![class(&[], &[])]);
    let chains = [
        (27, [23, 0]),
        (26, [23, 27]),
        (9, [50, 0]),
        (32, [50, 9]),
        (145, [144, 0]),
        (146, [144, 145]),
        (147, [146, 0]),
        (148, [147, 0]),
        (149, [147, 148]),
    ];
    for (power, needs) in chains {
        let needs = needs.map(|need| (need != 0).then_some(need));
        classes.prerequisites.insert(power, needs);
    }
    let powers = [23, 27, 26, 50, 9, 32, 144, 145, 146, 147, 148, 149];
    classes.classes[0].powers = powers.iter().map(|power| (1, *power)).collect();
    let class = &classes.classes[0];
    let made = classes.power_chart(class, &[]);
    let row = |powers: [u16; 3]| powers.map(Some);
    let expected = vec![
        row([23, 27, 26]),
        row([50, 9, 32]),
        row([144, 145, 146]),
        row([147, 148, 149]),
    ];
    let wave = classes.why_not_power(class, 26, &[23], 1);
    (
        "power_chart",
        check(
            made == expected && wave == Some(Unpickable::Needs(27)),
            format!("{made:?}, Wave after Push alone: {wave:?}"),
        ),
    )
}

/// A feat or power whose prerequisite is another pick of the same level,
/// as both games' level-up screens allow (TSL's executable reads a
/// prerequisite from the level's own picks too): a level with two
/// feats and two powers offers Improved Flurry with Flurry as its need and
/// Force Whirlwind with Force Push, and takes each pair in either order. A
/// level with Whirlwind alone is refused, naming Push.
fn same_level_picks() -> (&'static str, Result<String, String>) {
    let (flurry, improved, push, whirlwind) = (11, 91, 23, 27);
    let mut classes = classes(vec![class(&[flurry, improved], &[])]);
    classes.needs.insert(
        improved,
        Needs {
            feats: [Some(flurry), None],
            ..Needs::default()
        },
    );
    classes.prerequisites.insert(whirlwind, [Some(push), None]);
    let jedi = &mut classes.classes[0];
    jedi.feat_picks = vec![2; 20];
    jedi.power_picks = vec![2; 20];
    jedi.powers = vec![(1, push), (1, whirlwind)];
    let (sheet, career) = (soldier(&[]), vec![START]);
    let offered = classes.picks(&sheet, &career, 0).map(|picks| {
        (
            picks.feats,
            picks.feat_needs,
            picks.powers,
            picks.power_needs,
        )
    });
    let level = |feats: Vec<u16>, powers: Vec<u16>| Choices {
        class: 0,
        ability: Some(Ability::Strength as u8),
        feats,
        powers,
        ..Choices::default()
    };
    let both = classes
        .taken(
            &sheet,
            &career,
            &level(vec![improved, flurry], vec![whirlwind, push]),
            0,
        )
        .map(|(after, ..)| (after.feats.has(improved), after.powers.has(whirlwind)));
    let alone = classes
        .taken(&sheet, &career, &level(Vec::new(), vec![whirlwind]), 0)
        .err();
    let expected = (
        vec![flurry, improved],
        vec![(improved, vec![flurry])],
        vec![push, whirlwind],
        vec![(whirlwind, vec![push])],
    );
    (
        "same_level_picks",
        check(
            offered == Some(expected)
                && both == Ok((true, true))
                && alone == Some(Refusal::Power(whirlwind, Unpickable::Needs(push))),
            format!("offered {offered:?}, both pairs {both:?}, Whirlwind alone {alone:?}"),
        ),
    )
}

fn levels() -> (&'static str, Result<String, String>) {
    let toughness = feats::TOUGHNESS[0];
    let mut classes = classes(vec![
        class(&[toughness], &[]),
        class(&[], &[(1, feats::FORCE_SENSITIVE)]),
    ]);
    let take = |classes: &Classes, sheet: &Sheet, career: &Career, choices: &Choices| {
        classes
            .take(sheet, career, choices, 0)
            .map(|(sheet, ..)| sheet)
    };
    // Level 4 raises an ability: strength.
    let soldier_level = |feats| Choices {
        class: 0,
        ability: Some(Ability::Strength as u8),
        feats,
        ..Choices::default()
    };
    let career = vec![START];
    let picked = take(
        &classes,
        &soldier(&[]),
        &career,
        &soldier_level(vec![toughness]),
    );
    let wookiee = take(
        &classes,
        &soldier(&feats::WOOKIEE_TOUGHNESS[..1]),
        &career,
        &soldier_level(Vec::new()),
    );
    let jedi = take(
        &classes,
        &soldier(&[]),
        &vec![START, (1, 0)],
        &Choices {
            class: 1,
            ability: Some(Ability::Strength as u8),
            ..Choices::default()
        },
    );
    let level = 10 + 2;
    let ok = picked.is_some_and(|sheet| sheet.max_hp == HP + level + 1 + 3)
        && wookiee.is_some_and(|sheet| sheet.max_hp == HP + level + 2)
        && jedi.is_some_and(|sheet| sheet.max_fp == Sheet::dev_soldier().max_fp + 40);
    classes.needs.insert(
        toughness,
        Needs {
            abilities: [0, 0, CON + 1, 0, 0, 0],
            ..Needs::default()
        },
    );
    let short = take(
        &classes,
        &soldier(&[]),
        &career,
        &soldier_level(vec![toughness]),
    );
    let auto = classes
        .auto(&soldier(&[]), &career, 0, 0)
        .map(|choices| choices.feats);
    (
        "level_up_feats",
        check(
            ok && short.is_none() && auto == Some(Vec::new()),
            format!(
                "Toughness picked: HP {HP} → {:?}; Wookiee Toughness → {:?}; Force Sensitive's \
                 FP {:?}; needing CON {}: picked {:?}, the game's picks {auto:?}",
                picked.map(|sheet| sheet.max_hp),
                wookiee.map(|sheet| sheet.max_hp),
                jedi.map(|sheet| sheet.max_fp),
                CON + 1,
                short.map(|sheet| sheet.level)
            ),
        ),
    )
}
