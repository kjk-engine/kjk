//! The probes of the player character (`persona`, `creation`): for the
//! approved tests `player_creation`, `player_persona_scripts` and
//! `body_dressed_by_armour`, on the proving ground with a class, scripts
//! and items of our own; no install.

use super::SimWorld;
use crate::action::Action;
use crate::combat::sheet::races;
use crate::creation::{self, Creation, Refused};
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog, ItemFacts, PARTY, slots};
use crate::levels::{Choices, Class, Classes, Needs, Plan, SKILLS};
use crate::persona::{self, FEMALE, MALE, Persona};
use crate::probe::{Checks, check};
use crate::script::{MODULE, Pending};
use crate::{Snapshot, StepReason, WorldConfig};

/// The feat the class grants at level 1, those it may pick (recommended
/// first), and the one that needs `PICKED` first.
const GRANTED: u16 = 5;
const RECOMMENDED: u16 = 10;
const PICKED: u16 = 11;
const NEEDS_PICKED: u16 = 12;
/// The feat TSL's creation grants a player besides (`Class::pc_granted`).
const PC_GRANTED: u16 = 13;
/// The made character's persona's appearance and portrait.
const APPEARANCE: u16 = 95;
const PORTRAIT: u16 = 2;

/// One class: a d8, 4 skill points, the first four skills its own, a feat
/// granted and one picked at level 1.
fn classes() -> Classes {
    let class = Class {
        hit_die: 8,
        skill_points: 4,
        bab: (0..20).collect(),
        saves: vec![[1, 2, 0]; 20],
        defence: vec![0; 20],
        feat_picks: vec![1; 20],
        power_picks: vec![0; 20],
        granted: vec![(1, GRANTED)],
        pc_granted: vec![PC_GRANTED],
        feats: vec![RECOMMENDED, PICKED, NEEDS_PICKED],
        class_skills: std::array::from_fn(|skill| skill < 4),
        skill_order: (0..SKILLS as u8).collect(),
        ..Class::default()
    };
    let needs = Needs {
        feats: [Some(PICKED), None],
        ..Needs::default()
    };
    Classes {
        classes: vec![class.clone(), class],
        levels: vec![0, 1000, 3000, 6000],
        needs: [(NEEDS_PICKED, needs)].into(),
        ..Classes::default()
    }
}

fn persona(seat: Seat) -> Persona {
    Persona {
        seat,
        appearance: APPEARANCE,
        gender: FEMALE,
        portrait: PORTRAIT,
        name: "Mira".into(),
    }
}

/// STR 10, DEX 14, CON 14, INT 12, WIS 10, CHA 10 (22 points), 4 ranks in
/// each of the first two skills (8 points: (4 + 1) / 2 × 4), `feats`.
fn made(feats: Vec<u16>) -> Creation {
    let mut skills = [0; SKILLS];
    skills[..2].fill(4);
    Creation {
        persona: persona(Seat(0)),
        abilities: [10, 14, 14, 12, 10, 10],
        choices: Choices {
            class: 0,
            ability: None,
            skills,
            feats,
            powers: Vec::new(),
        },
    }
}

/// The proving ground with the class, `scripts` and `items`, one seat.
fn world(scripts: Vec<(&str, fmt_odyssey::ncs::Ncs)>, catalog: &str, items: Catalog) -> SimWorld {
    let ground = crate::determinism::proving_ground();
    let library = crate::probe::library(catalog, scripts)
        .with_items(items)
        .with_classes(classes());
    SimWorld::new(WorldConfig::one_seat(), ground.with_scripts(library))
        .expect("the proving ground makes a world")
}

/// Seat 0 standing a step, with `action` if given.
fn step(world: &mut SimWorld, action: Option<Action>) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    if let Some(action) = action {
        input.act(Seat(0), action);
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority)
}

/// Character creation's rules: scores bought with 30 points, the first
/// level a level of the class taken from nothing, refusals, and a seat's
/// player made so.
pub fn creation() -> Checks {
    let classes = classes();
    let mut checks: Checks = Vec::new();
    let costs = [
        [8; 6],
        [14, 8, 8, 8, 8, 8],
        [18, 8, 8, 8, 8, 8],
        [7, 8, 8, 8, 8, 8],
        [19, 8, 8, 8, 8, 8],
    ]
    .map(|scores| creation::spent(&scores));
    checks.push((
        "scores cost their modifiers from 8, 1 to 3 a raise, 8 to 18",
        check(
            costs == [Some(0), Some(6), Some(16), None, None],
            format!("8s, a 14, an 18, a 7, a 19: {costs:?}"),
        ),
    ));

    let first = creation::sheet(&classes, &made(vec![PICKED]), 2);
    checks.push((
        "the first level: d8 + CON, the class's rows, granted and picked feats, ranks",
        check(
            first.as_ref().is_ok_and(|(sheet, career, unspent)| {
                sheet.level == 1
                    && sheet.max_hp == 8 + 2
                    && sheet.saves == [1, 2, 0]
                    && sheet.feats.has(GRANTED)
                    && sheet.feats.has(PICKED)
                    && !sheet.feats.has(RECOMMENDED)
                    && sheet.skills[..2] == [4, 4]
                    && career == &vec![(0, 1)]
                    && *unspent == 0
            }),
            format!("{first:?}"),
        ),
    ));
    checks.push((
        "made with nothing worn or held, neutral, human, of its faction",
        check(
            first.as_ref().is_ok_and(|(sheet, ..)| {
                sheet.weapons == [None, None]
                    && sheet.items.iter().all(Option::is_none)
                    && sheet.good_evil == 50
                    && sheet.race == races::HUMAN
                    && sheet.faction == 2
            }),
            format!("{first:?}"),
        ),
    ));
    let games = creation::sheet(&classes, &made(Vec::new()), 2);
    checks.push((
        "a feat left unpicked is the game's (recommended first)",
        check(
            games
                .as_ref()
                .is_ok_and(|(sheet, ..)| sheet.feats.has(RECOMMENDED) && !sheet.feats.has(PICKED)),
            format!("{games:?}"),
        ),
    ));
    let refused = |change: fn(&mut Creation)| {
        let mut creation = made(vec![PICKED]);
        change(&mut creation);
        creation::sheet(&classes, &creation, 2).err()
    };
    let refusals = [
        refused(|c| c.abilities = [18, 18, 8, 8, 8, 8]),
        refused(|c| c.choices.skills[2] = 1),
        refused(|c| c.choices.skills = [5, 0, 0, 0, 0, 0, 0, 0]),
        refused(|c| c.choices.feats = vec![NEEDS_PICKED]),
        refused(|c| c.choices.feats = vec![RECOMMENDED, PICKED]),
    ];
    let expected = [
        Some(Refused::Scores),
        Some(Refused::Level),
        Some(Refused::Level),
        Some(Refused::Level),
        Some(Refused::Level),
    ];
    checks.push((
        "refused: 32 points, skills overspent, ranks past the cap, a feat's need, two picks of one",
        check(refusals == expected, format!("{refusals:?}")),
    ));

    // Issue 370: a mod's `classes.2da` recommended 90 in every score, and
    // the host refused the quick character the screen made of them.
    const SOLDIER: [u8; 6] = [16, 14, 14, 12, 10, 10];
    const MODDED: [u8; 6] = [90; 6];
    const LOW: [u8; 6] = [6, 8, 8, 8, 8, 8];
    let makeable = [
        (SOLDIER, SOLDIER),
        ([18, 18, 8, 8, 8, 8], SOLDIER),
        (MODDED, MODDED),
        ([89, 90, 90, 90, 90, 90], MODDED),
        ([91, 89, 90, 90, 90, 90], MODDED),
        ([18, 18, 18, 18, 18, 18], MODDED),
        (LOW, LOW),
        ([5, 8, 8, 8, 8, 8], LOW),
        ([6, 9, 8, 8, 8, 8], LOW),
        ([8, 8, 8, 8, 8, 8], LOW),
    ]
    .map(|(scores, recommended)| creation::makeable(&scores, &recommended));
    checks.push((
        "the screen's scores are made: 30 points, or the class's recommended, lowered for points to raise others",
        check(
            makeable == [true, false, true, true, false, true, true, false, false, true],
            format!("{makeable:?}"),
        ),
    ));
    let mut modded = classes.clone();
    modded.classes[0].recommended = MODDED;
    let mut quick = made(vec![PICKED]);
    quick.abilities = MODDED;
    let made_modded = creation::sheet(&modded, &quick, 2);
    checks.push((
        "a mod's recommended scores past 30 points are taken, as the original takes them",
        check(
            made_modded
                .as_ref()
                .is_ok_and(|(sheet, ..)| sheet.abilities == MODDED),
            format!("{made_modded:?}"),
        ),
    ));

    checks.push((
        "TSL's creation grants the class's player feats besides",
        check(
            first
                .as_ref()
                .is_ok_and(|(sheet, ..)| sheet.feats.has(PC_GRANTED)),
            format!("{first:?}"),
        ),
    ));
    checks.push(planned(&classes));
    checks.push(banked(&classes));
    checks.push(class_skill_feat(&classes));

    let mut world = world(Vec::new(), "", Catalog::default());
    let made_world = world.create(&made(vec![PICKED]));
    let after = step(&mut world, None);
    let me = after.body(Seat(0));
    let sheet = me.and_then(|entity| entity.sheet);
    let career = me.and_then(|entity| after.progress.careers.get(&entity.id.0));
    checks.push((
        "a seat's player made: its sheet, full hit points, career and persona",
        check(
            made_world.is_ok()
                && sheet.is_some_and(|sheet| sheet.level == 1 && sheet.max_hp == 10)
                && me
                    .and_then(|entity| entity.fighter)
                    .is_some_and(|fighter| fighter.whole_hp() == 10)
                && career == Some(&vec![(0, 1)])
                && persona::of(&after.personas, Seat(0)) == Some(&persona(Seat(0))),
            format!("{made_world:?}, sheet {sheet:?}, career {career:?}"),
        ),
    ));
    let mut nobody = made(Vec::new());
    nobody.persona.seat = Seat(3);
    let refused = world.create(&nobody);
    checks.push((
        "refused for a seat with no character",
        check(refused.is_err(), format!("{refused:?}")),
    ));
    checks
}

/// A build's plan picks a level before the class's order: its skills
/// first (the level's 2 points all on the first), its feat, its ability at the fourth level; the game's picks
/// (`auto`) are the plan left empty.
fn planned(classes: &Classes) -> (&'static str, Result<String, String>) {
    let made = creation::sheet(classes, &made(vec![PICKED]), 2);
    let Ok((sheet, career, _)) = made else {
        return ("a level-up's planned picks", Err(format!("{made:?}")));
    };
    let plan = Plan {
        skills: vec![3, 2],
        feats: vec![NEEDS_PICKED],
        powers: Vec::new(),
        abilities: vec![4],
        bank: false,
    };
    let picked = classes.planned(&sheet, &career, 0, &plan, 0);
    let auto = classes.auto(&sheet, &career, 0, 0);
    let fourth = crate::combat::Sheet { level: 3, ..sheet };
    let raised = classes
        .planned(&fourth, &career, 0, &plan, 0)
        .and_then(|c| c.ability);
    (
        "a level-up's planned picks: the plan's skills and feat first, its ability at the fourth level",
        check(
            picked.as_ref().is_some_and(|c| {
                c.skills[3] == 2
                    && c.skills[2] == 0
                    && c.skills[0] == 0
                    && c.feats == [NEEDS_PICKED]
            }) && auto
                .as_ref()
                .is_some_and(|c| c.skills[0] == 1 && c.feats == [RECOMMENDED])
                && raised == Some(4),
            format!("{picked:?}, the game's {auto:?}, the fourth level raises {raised:?}"),
        ),
    )
}

/// A plan that banks: the second level's 2 points on skill 0 alone, which
/// takes one rank to its most (5), the other kept (both games keep a
/// level's unspent points, TSL's `CSWCLevelUpStats::AddSkillPointsPerLevel`
/// adding the next level's to them); the third level spends its 2 and the
/// kept one on skill 3; the 3 points refused without the one kept.
fn banked(classes: &Classes) -> (&'static str, Result<String, String>) {
    let made = creation::sheet(classes, &made(vec![PICKED]), 2);
    let Ok((sheet, career, _)) = made else {
        return ("unspent skill points kept", Err(format!("{made:?}")));
    };
    let only = |skill| Plan {
        skills: vec![skill],
        bank: true,
        ..Plan::default()
    };
    let second = classes.planned(&sheet, &career, 0, &only(0), 0);
    let taken = second
        .as_ref()
        .and_then(|choices| classes.take(&sheet, &career, choices, 0));
    let Some((after, career, kept)) = taken else {
        return ("unspent skill points kept", Err(format!("{second:?}")));
    };
    let third = classes.planned(&after, &career, 0, &only(3), kept);
    let spent = |banked| {
        third
            .as_ref()
            .and_then(|choices| classes.take(&after, &career, choices, banked))
            .map(|(.., left)| left)
    };
    (
        "unspent skill points kept: a plan that banks keeps the rest, the next level spends it with its own",
        check(
            second
                .as_ref()
                .is_some_and(|c| c.skills == [1, 0, 0, 0, 0, 0, 0, 0])
                && kept == 1
                && third.as_ref().is_some_and(|c| c.skills[3] == 3)
                && spent(kept) == Some(0)
                && spent(0).is_none(),
            format!(
                "second {second:?}, kept {kept}, third {third:?}, left {:?}, without the kept {:?}",
                spent(kept),
                spent(0)
            ),
        ),
    )
}

/// TSL's Class Skill feats: Security (skill 6, not the class's own) costs
/// a point a rank to a character with Class Skill: Security, so the second
/// level's 2 points buy 2 ranks; without the feat they buy none. The
/// level's picks (the level-up screen's) say the same.
fn class_skill_feat(classes: &Classes) -> (&'static str, Result<String, String>) {
    const SECURITY: usize = 6;
    let made = creation::sheet(classes, &made(vec![PICKED]), 2);
    let Ok((sheet, career, _)) = made else {
        return ("a Class Skill feat", Err(format!("{made:?}")));
    };
    let feat = crate::levels::CLASS_SKILL_FEATS[SECURITY].expect("Security's feat");
    let featured = crate::combat::Sheet {
        feats: sheet.feats.with(feat),
        ..sheet
    };
    let mut choices = Choices {
        class: 0,
        ability: None,
        skills: [0; SKILLS],
        feats: Vec::new(),
        powers: Vec::new(),
    };
    choices.skills[SECURITY] = 2;
    let left = |sheet: &crate::combat::Sheet| {
        classes
            .take(sheet, &career, &choices, 0)
            .map(|(after, _, left)| (after.skills[SECURITY], left))
    };
    let (with, without) = (left(&featured), left(&sheet));
    // The level-up screen's class skills are the same set's.
    let shown = |sheet: &crate::combat::Sheet| {
        classes
            .picks(sheet, &career, 0)
            .map(|picks| picks.class_skills[SECURITY])
    };
    let shown = (shown(&featured), shown(&sheet));
    (
        "Class Skill: Security makes Security a class skill: a point a rank",
        check(
            with == Some((2, 0)) && without.is_none() && shown == (Some(true), Some(false)),
            format!("with the feat {with:?}, without {without:?}; picks show {shown:?}"),
        ),
    )
}

/// The script `probe_xp`: `GiveXPToCreature(GetFirstPC(), 500)`, then
/// `AddMultiClass(1, GetFirstPC())`.
fn xp_script() -> (Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, &'static str) {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
object GetFirstPC();
// 1
void GiveXPToCreature(object oCreature, int nGiveXPAmount);
// 2
void AddMultiClass(int nClassType, object oSource);
"#;
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Int(500)))
        .op(call(0, 0))
        .op(call(1, 2))
        .op(call(0, 0))
        .op(Op::Const(Const::Int(1)))
        .op(call(2, 2))
        .op(Op::Retn);
    (vec![("probe_xp", asm.build())], CATALOG)
}

/// TSL's player levels as its executable keeps them: XP the party gains
/// tells the scripts the level it reaches (`G_PC_LEVEL`,
/// `CSWSCreature::ReceiveExperience`), and `AddMultiClass` gives the player
/// the class with no levels and the party the XP its next level asks
/// (`ExecuteCommandAddMultiClass`), so the next level-up is of that class.
pub fn levels_told() -> Checks {
    let (scripts, catalog) = xp_script();
    let mut world = world(scripts, catalog, Catalog::default());
    let made = world.create(&made(vec![PICKED]));
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new("probe_xp", MODULE));
    world.set_script_state(state);
    let after = step(&mut world, None);
    let state = world.script_state();
    let level = crate::script::global(&state.numbers, crate::script::xp::PC_LEVEL).copied();
    let xp = state.xp(PARTY);
    let career = after
        .body(Seat(0))
        .and_then(|me| state.careers.get(&me.id.0))
        .cloned();
    vec![(
        "XP received tells G_PC_LEVEL its level; AddMultiClass gives the class unlevelled and the next level's XP",
        check(
            made.is_ok() && level == Some(2) && xp == 1000 && career == Some(vec![(0, 1), (1, 0)]),
            format!("{made:?}: G_PC_LEVEL {level:?}, XP {xp}, career {career:?}"),
        ),
    )]
}

/// The routines the scripts of `class_at_once` and `bench_holds` call.
const BENCH_CATALOG: &str = r#"
// 0
object GetFirstPC();
// 1
void ShowUpgradeScreen(object oItem);
// 2
void DelayCommand(float fSeconds, action aActionToDelay);
// 3
void SetLocalNumber(object oObject, int nIndex, int nValue);
// 4
void AddMultiClass(int nClassType, object oSource);
// 5
int GetClassByPosition(int nClassPosition, object oCreature);
"#;

/// The local number the probes' scripts set on the player.
const TOLD: i32 = 2;

/// `SetLocalNumber(GetFirstPC(), TOLD, <the value on the stack>)`.
fn tell_player(asm: &mut script_ncs::probes::Asm) {
    use fmt_odyssey::ncs::{Const, Op};
    asm.op(Op::Const(Const::Int(TOLD)))
        .op(Op::Action {
            routine: 0,
            args: 0,
        })
        .op(Op::Action {
            routine: 3,
            args: 3,
        });
}

/// The player's local number `TOLD`, as the probes' scripts left it.
fn player_local(world: &SimWorld, me: Option<u32>) -> Option<i32> {
    let me = me?;
    world.script_state().locals.get(&(me, 1, TOLD)).copied()
}

/// A class `AddMultiClass` gives counts at once: the same script's
/// `GetClassByPosition(2, …)` answers it before its first level, as both
/// games' do (a mod's start script picks the Jedi's robes by it, its
/// level-up still open).
pub fn class_at_once() -> Checks {
    use fmt_odyssey::ncs::{Const, Op};
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Action {
        routine: 0,
        args: 0,
    })
    .op(Op::Const(Const::Int(1)))
    .op(Op::Action {
        routine: 4,
        args: 2,
    })
    .op(Op::Action {
        routine: 0,
        args: 0,
    })
    .op(Op::Const(Const::Int(2)))
    .op(Op::Action {
        routine: 5,
        args: 2,
    });
    tell_player(&mut asm);
    asm.op(Op::Retn);
    let mut world = world(
        vec![("probe_class", asm.build())],
        BENCH_CATALOG,
        Catalog::default(),
    );
    let made = world.create(&made(vec![PICKED]));
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new("probe_class", MODULE));
    world.set_script_state(state);
    let after = step(&mut world, None);
    let class = player_local(&world, after.body(Seat(0)).map(|me| me.id.0));
    vec![(
        "AddMultiClass's class is the second class at once, with no level in it yet",
        check(
            made.is_ok() && class == Some(1),
            format!("{made:?}: GetClassByPosition(2) {class:?}"),
        ),
    )]
}

/// A script that shows the upgrade bench and delays code after it: the code
/// waits while the bench is up and runs as long after it closes as it was
/// asked for, as both games pause under the bench (a mod's workbench takes
/// back the crystals left over only once the player has fitted one).
pub fn bench_holds() -> Checks {
    use fmt_odyssey::ncs::{Const, Op};
    // `ShowUpgradeScreen(OBJECT_INVALID); DelayCommand(0.1,
    // SetLocalNumber(GetFirstPC(), TOLD, 5))`.
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Object(1)))
        .op(Op::Action {
            routine: 1,
            args: 1,
        })
        .op(Op::StoreState { bp: 0, sp: 0 })
        .to(Op::Jmp, "delay")
        .op(Op::Const(Const::Int(5)));
    tell_player(&mut asm);
    asm.op(Op::Retn)
        .label("delay")
        .op(Op::Const(Const::Float(0.1)))
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Retn);
    let mut world = world(
        vec![("probe_bench", asm.build())],
        BENCH_CATALOG,
        Catalog::default(),
    );
    let made = world.create(&made(vec![PICKED]));
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new("probe_bench", MODULE));
    world.set_script_state(state);
    let mut last = step(&mut world, None);
    let me = last.body(Seat(0)).map(|me| me.id.0);
    let run_for = |world: &mut SimWorld, last: &mut Snapshot, ms: u64| {
        let until = last.time_ms + ms;
        while last.time_ms < until {
            *last = step(world, None);
        }
    };
    run_for(&mut world, &mut last, 1000);
    let while_up = player_local(&world, me);
    step(&mut world, Some(Action::ScreenClosed));
    let closed = player_local(&world, me);
    run_for(&mut world, &mut last, 200);
    let after = player_local(&world, me);
    vec![(
        "code delayed after the bench shows waits until the first seat closes it, then its delay",
        check(
            made.is_ok() && while_up.is_none() && closed.is_none() && after == Some(5),
            format!("{made:?}: up a second {while_up:?}, at the close {closed:?}, after {after:?}"),
        ),
    )]
}

/// A creature outside the party running `AddMultiClass(1, OBJECT_SELF)` on
/// a module of `game`: its career and XP, and the party's XP. K1's
/// `ExecuteCommandAddMultiClass` gives any creature the class unlevelled
/// and the party the XP its next level asks; TSL's gives one outside the
/// party that XP itself and levels it at once (`AutoLevelup`).
fn multiclassed(game: &str) -> Result<(Option<crate::levels::Career>, i32, i32), String> {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void AddMultiClass(int nClassType, object oSource);
"#;
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = game.into();
    let first = data.placements.len() as u32;
    let at = super::talk_end_probes::by_entry(2.0, 0.0);
    data.placements.push(crate::module::Placement::Own(
        crate::module::ObjectData::creature(
            "stranger",
            "stranger",
            at,
            crate::combat::Sheet::commoner(),
        ),
    ));
    let library = crate::probe::library(CATALOG, [("probe_multiclass", asm.build())])
        .with_items(Catalog::default())
        .with_classes(classes());
    let level = ground.with_module(data).with_scripts(library);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let stranger = super::talk_end_probes::id_of(&mut world, first)?;
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(
        "probe_multiclass",
        script_ncs::vm::ObjectId(stranger),
    ));
    world.set_script_state(state);
    step(&mut world, None);
    let state = world.script_state();
    Ok((
        state.careers.get(&stranger).cloned(),
        state.xp(stranger),
        state.xp(PARTY),
    ))
}

/// `multiclassed` in each game.
pub fn multiclass_outside_party() -> Checks {
    let k1 = multiclassed("k1");
    let tsl = multiclassed("tsl");
    vec![
        (
            "K1's AddMultiClass: a stranger's class unlevelled, the party given its next level's XP",
            check(
                matches!(&k1, Ok((Some(career), 0, party)) if career.last() == Some(&(1, 0)) && *party > 0),
                format!("career, own XP, party XP: {k1:?}"),
            ),
        ),
        (
            "TSL's AddMultiClass: a stranger given the XP itself and a level of the class at once",
            check(
                matches!(&tsl, Ok((Some(career), own, 0)) if career.last() == Some(&(1, 1)) && *own > 0),
                format!("career, own XP, party XP: {tsl:?}"),
            ),
        ),
    ]
}

/// The script `probe_persona`, setting custom tokens: 1 `GetGender`, 2
/// `GetName`, 3 `GetAppearanceType`, each of `GetFirstPC()`.
fn persona_script() -> (Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, &'static str) {
    use fmt_odyssey::ncs::{Const, Op};
    const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
int GetGender(object oCreature);
// 3
string GetName(object oObject);
// 4
int GetAppearanceType(object oCreature);
// 5
object GetFirstPC();
"#;
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let mut asm = script_ncs::probes::Asm::default();
    for (token, routine, text) in [(1, 2, false), (2, 3, true), (3, 4, false)] {
        asm.op(call(5, 0)).op(call(routine, 1));
        if !text {
            asm.op(call(1, 1));
        }
        asm.op(Op::Const(Const::Int(token))).op(call(0, 2));
    }
    asm.op(Op::Retn);
    (vec![("probe_persona", asm.build())], CATALOG)
}

/// What `probe_persona` tells of seat 0's player in `world`.
fn told(world: &mut SimWorld) -> Vec<String> {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new("probe_persona", MODULE));
    world.set_script_state(state);
    let after = step(world, None);
    (1..=3)
        .map(|n| {
            after
                .talk
                .tokens
                .get(&n)
                .cloned()
                .unwrap_or_else(String::new)
        })
        .collect()
}

/// The scripts read a player's persona of its own character; a snapshot
/// and a world made from it keep it.
pub fn persona_scripts() -> Checks {
    let mut checks: Checks = Vec::new();
    let (scripts, catalog) = persona_script();
    let mut given = world(scripts.clone(), catalog, Catalog::default());
    given.set_persona(persona(Seat(0)));
    let seen = told(&mut given);
    checks.push((
        "GetGender, GetName, GetAppearanceType of a player read its persona",
        check(
            seen == ["1", "Mira", &APPEARANCE.to_string()],
            format!("{seen:?}"),
        ),
    ));
    let mut man = world(scripts.clone(), catalog, Catalog::default());
    man.set_persona(Persona {
        gender: MALE,
        ..persona(Seat(0))
    });
    let seen = told(&mut man);
    checks.push((
        "a man's persona: GetGender 0",
        check(
            seen.first().map(String::as_str) == Some("0"),
            format!("{seen:?}"),
        ),
    ));
    let mut none = world(scripts, catalog, Catalog::default());
    let seen = told(&mut none);
    checks.push((
        "a player with no persona: male, no name, no appearance",
        check(seen == ["0", "", "0"], format!("{seen:?}")),
    ));
    let snapshot = given.snapshot();
    let decoded = Snapshot::decode(&snapshot.encode());
    let again = SimWorld::from_snapshot(&snapshot, given.level().clone())
        .map(|mut world| world.snapshot().personas);
    checks.push((
        "the persona rides in a snapshot and the world made from it",
        check(
            decoded
                .as_ref()
                .is_ok_and(|decoded| decoded.personas == snapshot.personas)
                && again
                    .as_ref()
                    .is_ok_and(|personas| *personas == snapshot.personas)
                && !snapshot.personas.is_empty(),
            format!("{:?}", snapshot.personas),
        ),
    ));
    checks
}

/// The body armour seat 0 wears is what its body is dressed as
/// (`persona::dressed`): none, then a robe it puts on, then none again.
pub fn dressing() -> Checks {
    let robe = ItemFacts {
        slots: 1 << slots::BODY,
        ..super::containers::plain("robe")
    };
    let mut world = world(Vec::new(), "", Catalog::new([robe.clone()]));
    world.set_persona(persona(Seat(0)));
    let mut state = world.script_state().clone();
    let item = state.make_item(&Catalog::new([robe]), "robe", PARTY, 1, None);
    world.set_script_state(state);
    let before = step(&mut world, None);
    let bare = persona::dressed(&before, Seat(0));
    let mut checks: Checks = vec![(
        "dressed in its underclothes at first",
        check(bare == Some((APPEARANCE, None)), format!("{bare:?}")),
    )];
    let Some(item) = item else {
        checks.push(("the robe is made", Err("no robe".into())));
        return checks;
    };
    let equip = Action::Equip {
        item: item.0,
        slot: slots::BODY,
    };
    step(&mut world, Some(equip));
    let worn = step(&mut world, None);
    let dressed = persona::dressed(&worn, Seat(0));
    checks.push((
        "putting on a body item dresses the body in it",
        check(
            dressed == Some((APPEARANCE, Some("robe".into()))),
            format!("{dressed:?}"),
        ),
    ));
    step(&mut world, Some(Action::Unequip(slots::BODY)));
    let off = step(&mut world, None);
    let undressed = persona::dressed(&off, Seat(0));
    checks.push((
        "taking it off leaves the underclothes",
        check(
            undressed == Some((APPEARANCE, None)),
            format!("{undressed:?}"),
        ),
    ));
    let nobody = persona::dressed(&off, Seat(1));
    checks.push((
        "a seat with no character is dressed as nothing",
        check(nobody.is_none(), format!("{nobody:?}")),
    ));
    checks
}
