//! The probes of the powers only their impact scripts cast
//! (`force::scripted`; `kjk sim probes --only "power rows"`), on the proving
//! ground, no install:
//! - a Jedi's scripted power from a foe's menu reaches as far as its row's
//!   range, and its impact script's own sphere round the one aimed at
//!   decides whom it reaches: a foe beside the one aimed at, inside the
//!   sphere, is hurt, and one outside it is not; aimed past its range, it
//!   reaches no one;
//! - the cast shows the row's conjure clip, then its looped clip, with the
//!   hand busy for the cast time;
//! - a creature that knows only a scripted power from a foe's menu casts it
//!   at the player when its script chooses it, and a script's choice of
//!   such a power is pressed (`ai::power_cast`);
//! - the minds' reach of a scripted power is its row's range, or anywhere
//!   for one from its own menu;
//! - a sheet that knows power and feat row 400 survives a snapshot, and an
//!   old one loads;
//! - the Force routines mods call are bound, and a call of an unbound
//!   routine warns once;
//! - Force Aura and Resist Force, both cast through their scripts, run at
//!   the same time.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;

use super::{Fighting, Id, Player, SimWorld};
use crate::combat::force::{self, Gesture};
use crate::combat::sheet::{Rows, factions, flags};
use crate::combat::{Fighter, Sheet, ai};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::{Menu, Talent, Talents};
use crate::snapshot::{Kind, SimId};
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

/// The row cast: TSL's Crush Opposition I, a power JA has no shape for.
const ROW: u16 = 144;

/// What `ROW` costs (whole FP).
const COST: u16 = 10;

/// The impact script's sphere round the one aimed at (metres).
const SPHERE_M: f32 = 2.0;

/// What the impact script takes from each one it finds (whole HP).
const DAMAGE: i32 = 5;

/// The row's cast: its conjure clip, its looped clip (`castout1` and
/// `castoutlp1` as `asset_odyssey::creature::cast_rows` numbers them), and
/// their times.
const CONJURE_CLIP: u16 = 10_062;
const LOOP_CLIP: u16 = 20_063;
const CONJURE_MS: u16 = 200;
const CAST_MS: u16 = 1000;

/// Where the Jedi stands (metres, the ground's own axes), facing +X; the
/// foe it aims at, one beside that foe inside the sphere, and one beside it
/// outside the sphere (but within the 20 m the old delivery caught).
const JEDI_AT: (f32, f32) = (-7.0, 0.0);
const FOES_AT: [(f32, f32); 3] = [(-4.5, 0.0), (-4.5, 1.5), (-4.5, -4.0)];

const CATALOG: &str = r#"
int SHAPE_SPHERE = 4;
int OBJECT_TYPE_CREATURE = 1;
int DURATION_TYPE_INSTANT = 0;
int DAMAGE_TYPE_UNIVERSAL = 8;
// 0
object GetFirstObjectInShape(int nShape, float fSize, location lTarget, int bLineOfSight = FALSE, int nObjectFilter = OBJECT_TYPE_CREATURE, vector vOrigin = [0.0,0.0,0.0]);
// 1
object GetNextObjectInShape(int nShape, float fSize, location lTarget, int bLineOfSight = FALSE, int nObjectFilter = OBJECT_TYPE_CREATURE, vector vOrigin = [0.0,0.0,0.0]);
// 2
location GetSpellTargetLocation();
// 3
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 4
effect EffectDamage(int nDamageAmount, int nDamageType = DAMAGE_TYPE_UNIVERSAL, int nDamagePower = 0);
// 5
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 6
talent TalentSpell(int nSpell);
// 7
void ActionUseTalentOnObject(talent tChosenTalent, object oTarget);
// 8
object GetFirstPC();
// 9
void PrintInteger(int nInteger);
"#;

/// A routine no binding carries out, which the creature's heartbeat calls
/// twice.
const UNBOUND: &str = "PrintInteger";

/// A creature's heartbeat that calls `UNBOUND` twice, then uses `ROW` on
/// the player, as KOTOR's AI scripts use a talent they chose.
fn choice() -> Result<fmt_odyssey::ncs::Ncs, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let talent = call("TalentSpell", [Expr::Int(i32::from(ROW))]);
    let unbound = || call(UNBOUND, [Expr::Int(1)]);
    script(
        &catalog,
        &[
            unbound(),
            unbound(),
            call("ActionUseTalentOnObject", [talent, call("GetFirstPC", [])]),
        ],
    )
}

/// The impact: `DAMAGE` to each creature its sphere round the spell's
/// target finds (the first five, written out, as the probe's assembler has
/// no loops), and custom token 7 set to show it ran.
fn impact() -> Result<fmt_odyssey::ncs::Ncs, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let shape = |routine: &str| {
        call(
            routine,
            [
                Expr::Int(4),
                Expr::Float(SPHERE_M),
                call("GetSpellTargetLocation", []),
                Expr::Int(0),
                Expr::Int(1),
                Expr::Vector([0.0; 3]),
            ],
        )
    };
    let hurt = |found: Expr| {
        call(
            "ApplyEffectToObject",
            [
                Expr::Int(0),
                call(
                    "EffectDamage",
                    [Expr::Int(DAMAGE), Expr::Int(8), Expr::Int(0)],
                ),
                found,
                Expr::Float(0.0),
            ],
        )
    };
    let mut statements = vec![hurt(shape("GetFirstObjectInShape"))];
    statements.extend((0..4).map(|_| hurt(shape("GetNextObjectInShape"))));
    statements.push(call(
        "SetCustomToken",
        [Expr::Int(7), Expr::String("1".into())],
    ));
    script(&catalog, &statements)
}

/// A point of the ground (metres) as a body's origin there (units).
fn origin(at: (f32, f32)) -> crate::math::Vec3 {
    crate::script::standing(crate::determinism::ground_point(at.0, at.1, 0.0))
}

/// A world on the proving ground (TSL's rules) whose `ROW` is a scripted
/// power from a foe's menu reaching `reach_dm` (`Talent::reach_dm`), with
/// the impact `impact`, the player `player` at `JEDI_AT`, and a hostile
/// creature with `foe` at each of `foes`, whose heartbeat uses `ROW` on the
/// player if `chooses`.
fn world(
    reach_dm: u16,
    player: Sheet,
    (foe, foes, chooses): (Sheet, &[(f32, f32)], bool),
) -> Result<(SimWorld, SimId, Vec<SimId>), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = "tsl".into();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    let size = usize::from(ROW) + 1;
    data.force_costs = vec![0; size];
    data.force_costs[usize::from(ROW)] = COST;
    for (n, at) in foes.iter().enumerate() {
        let scripts = if chooses {
            vec![("ScriptHeartbeat".into(), "probe_choose".into())]
        } else {
            Vec::new()
        };
        data.placements.push(Placement::Own(ObjectData {
            sheet: Some(foe),
            template: format!("foe{n}"),
            scripts,
            ..ObjectData::bare(
                ObjectKind::Creature,
                &format!("foe{n}"),
                crate::determinism::ground_point(at.0, at.1, 0.0),
            )
        }));
    }
    let mut spells = vec![Talent::default(); size];
    spells[usize::from(ROW)] = Talent {
        menu: Menu::Hostile,
        hostile: true,
        reach_dm,
        maxcr: 3,
        cast_ms: CAST_MS,
        conjure_ms: CONJURE_MS,
        cast_clips: [CONJURE_CLIP, LOOP_CLIP],
        ..Talent::default()
    };
    let mut impacts = vec![String::new(); size];
    impacts[usize::from(ROW)] = "probe_crush".into();
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let scripts = [("probe_crush", impact()?), ("probe_choose", choice()?)];
    let library = crate::probe::library_with(CATALOG, scripts, talents);
    let level = ground.with_module(data).with_scripts(library);
    let config = WorldConfig {
        sheet: player,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
    let start = world.snapshot();
    let jedi = start.body(Seat(0)).ok_or("no player")?.id;
    let mut foes: Vec<SimId> = start
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .map(|entity| entity.id)
        .collect();
    foes.sort();
    let mut bodies = world.world.query::<(&Id, &mut Player)>();
    for (id, mut body) in bodies.iter_mut(&mut world.world) {
        if id.0 == jedi {
            let at = origin(JEDI_AT);
            body.0.origin[0] = at[0];
            body.0.origin[1] = at[1];
        }
    }
    Ok((world, jedi, foes))
}

fn step(world: &mut SimWorld, command: Command) {
    let mut input = TickInput::default();
    input.insert(Seat(0), command);
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority);
}

fn fighter(world: &mut SimWorld, who: SimId) -> Option<Fighter> {
    let mut fighters = world.world.query::<(&Id, &Fighting)>();
    fighters
        .iter(&world.world)
        .find(|(id, _)| id.0 == who)
        .map(|(_, fighting)| fighting.0)
}

fn now(world: &SimWorld) -> u64 {
    world.world.resource::<super::Clock>().time_ms
}

/// A TSL Jedi who knows only `ROW`, and cannot die.
fn caster() -> Sheet {
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = Rows::default().with(ROW);
    jedi.flags |= flags::TSL | flags::IMMORTAL;
    jedi
}

/// A hostile soldier.
fn soldier() -> Sheet {
    Sheet {
        faction: factions::HOSTILE_1,
        ..Sheet::dev_soldier()
    }
}

/// The Jedi casts `ROW` reaching `reach_dm` at the three foes: whether each
/// was hurt, and the Jedi's clip and hand at the press, after the conjuring
/// and after the cast.
#[derive(Debug, Default)]
struct Seen {
    hurt: Vec<bool>,
    pressed: Option<(u16, Option<Gesture>)>,
    looping: Option<u16>,
    hand_free_after: bool,
}

fn cast_at_three(reach_dm: u16) -> Result<Seen, String> {
    let (mut world, jedi, foes) = world(reach_dm, caster(), (soldier(), &FOES_AT, false))?;
    step(&mut world, Command::default());
    let before: Vec<i32> = foes
        .iter()
        .map(|foe| fighter(&mut world, *foe).map_or(0, |f| f.hp))
        .collect();
    let press = Command {
        force: ROW,
        buttons: buttons::FORCE_POWER,
        ..Command::default()
    };
    step(&mut world, press);
    let mut seen = Seen::default();
    let start = now(&world);
    seen.pressed = fighter(&mut world, jedi).map(|f| {
        (
            f.acting.map_or(0, |acting| acting.animation),
            f.force.hand.map(|(gesture, _)| gesture),
        )
    });
    while now(&world) < start + u64::from(CAST_MS) + u64::from(MSEC) {
        step(&mut world, press.idle());
        let at = now(&world);
        if seen.looping.is_none() && at >= start + u64::from(CONJURE_MS) {
            seen.looping = fighter(&mut world, jedi)
                .and_then(|f| f.acting)
                .map(|acting| acting.animation);
        }
    }
    seen.hand_free_after = fighter(&mut world, jedi).is_some_and(|f| f.force.hand.is_none());
    seen.hurt = foes
        .iter()
        .zip(before)
        .map(|(foe, hp)| fighter(&mut world, *foe).is_some_and(|f| f.hp < hp))
        .collect();
    Ok(seen)
}

/// One cast of `ROW` reaching `reach_dm`: the foes `hurt` expects hurt, and
/// the cast's clips as the row gives them.
fn hostile_shape(reach_dm: u16, hurt: [bool; 3]) -> Result<String, String> {
    let seen = cast_at_three(reach_dm)?;
    let clips = seen.pressed == Some((CONJURE_CLIP, Some(Gesture::Cast)))
        && seen.looping == Some(LOOP_CLIP)
        && seen.hand_free_after;
    // Past its range nothing is aimed at: no clips, the hand JA's.
    let shown = if hurt[0] { clips } else { true };
    check(
        seen.hurt == hurt && shown,
        format!("reach {reach_dm} dm: {seen:?}"),
    )
}

/// A creature that knows only `ROW` casts it at the player when its script
/// chooses it: its impact runs (custom token 7) within `WITHIN_MS`, paid.
/// Its script called `UNBOUND` twice before it chose the power: the one
/// warning names the routine and the script, and the script went on.
fn creature_casts() -> Checks {
    let (cast, warned) = match creature_run() {
        Ok((cast_at, paid)) => {
            let warning = crate::script::unbound::warning(UNBOUND);
            let named = warning
                .as_deref()
                .is_some_and(|line| line.contains(UNBOUND) && line.contains("probe_choose"));
            (
                check(
                    cast_at.is_some() && paid,
                    format!("the impact ran at {cast_at:?} ms, paid {paid}"),
                ),
                check(
                    named && cast_at.is_some(),
                    format!("warned {warning:?}; the script went on to cast: {cast_at:?}"),
                ),
            )
        }
        Err(why) => (Err(why.clone()), Err(why)),
    };
    vec![
        ("power_rows_creature_casts", cast),
        ("power_rows_unbound_routine_warns", warned),
    ]
}

/// `creature_casts`' run: when the impact ran, and whether it was paid.
fn creature_run() -> Result<(Option<u64>, bool), String> {
    const WITHIN_MS: u64 = 20_000;
    let mut foe = caster();
    foe.faction = factions::HOSTILE_1;
    foe.flags &= !flags::IMMORTAL;
    let mut player = Sheet::dev_soldier();
    player.flags |= flags::IMMORTAL;
    let (mut world, _, foes) = world(100, player, (foe, &FOES_AT[..1], true))?;
    let caster = *foes.first().ok_or("no creature")?;
    let full = fighter(&mut world, caster).map_or(0, |f| f.fp);
    let mut cast_at = None;
    while now(&world) < WITHIN_MS && cast_at.is_none() {
        step(&mut world, Command::default());
        let ran = world
            .snapshot()
            .talk
            .tokens
            .get(&7)
            .is_some_and(|t| t == "1");
        cast_at = ran.then(|| now(&world));
    }
    let paid = fighter(&mut world, caster).map_or(0, |f| f.fp) < full;
    Ok((cast_at, paid))
}

/// The minds' own reading of a scripted row: a script's choice is pressed,
/// and the reach is the row's range (or anywhere from its own menu).
fn minds_read_rows() -> Result<String, String> {
    let reach_dm = 100;
    let talent = |menu: Menu| Talent {
        menu,
        reach_dm,
        ..Talent::default()
    };
    let hostile = |row: u16| (row == ROW).then(|| talent(Menu::Hostile));
    let friendly = |row: u16| (row == ROW).then(|| talent(Menu::Friendly));
    let sheet = caster();
    let fighter = Fighter::of(&sheet);
    let free = |_: u16| Some(0);
    let pressed = ai::power_cast(ROW, &sheet, &fighter, &free, &hostile);
    let unknown = ai::power_cast(ROW, &sheet, &fighter, &free, &ai::no_scripted);
    let reach = ai::power_reach(ROW, &hostile);
    let anywhere = ai::power_reach(ROW, &friendly);
    let range = force::scripted_reach(talent(Menu::Hostile));
    check(
        pressed == Some(ai::Cast::Press(ROW))
            && unknown.is_none()
            && reach == range
            && anywhere == f32::MAX,
        format!(
            "pressed {pressed:?}, without scripts {unknown:?}; reach {reach} (row {range}), \
             own menu {anywhere}"
        ),
    )
}

/// The routines a Force power's impact script may call that were left
/// unbound before are bound now.
fn force_routines_bound() -> Result<String, String> {
    const ROUTINES: [&str; 17] = [
        "EffectConfused",
        "EffectFrightened",
        "EffectForceResistanceDecrease",
        "EffectSeeInvisible",
        "MagicalEffect",
        "SupernaturalEffect",
        "ExtraordinaryEffect",
        "VersusAlignmentEffect",
        "VersusRacialTypeEffect",
        "GetEffectSubType",
        "GetEffectDurationType",
        "GetHasFeatEffect",
        "GetAreaOfEffectCreator",
        "TouchAttackMelee",
        "TouchAttackRanged",
        "SetForcePowerUnsuccessful",
        "GetSpellCastItem",
    ];
    let unbound: Vec<&str> = ROUTINES
        .into_iter()
        .filter(|name| !crate::script::bound(name))
        .collect();
    check(unbound.is_empty(), format!("still unbound: {unbound:?}"))
}

/// `sheet` written as a snapshot of `versions` writes it (its sheet, picks
/// and action bar), then read back as such a snapshot: what was read, and
/// whether every byte was.
fn sheet_bytes(sheet: &Sheet, versions: crate::wire::Versions) -> Result<Sheet, String> {
    let mut out = crate::wire::Writer::default();
    sheet.encode_as(&mut out, versions);
    let player = versions.snapshot >= crate::since::snapshot::AI_CHOICES.1;
    sheet.picks.encode_as(&mut out, player);
    sheet.bar.encode_as(&mut out, versions);
    let mut input = crate::wire::Reader::new("probe", &out.0).with_versions(versions);
    let read = crate::snapshot::decode_sheet(&mut input).map_err(|e| format!("{e:?}"))?;
    input.finish().map_err(|e| format!("{e:?}"))?;
    Ok(read)
}

/// A sheet that knows power row 400 and feat row 400, and whose items give
/// feat row 450, comes back whole from this build's snapshot. One written
/// as a snapshot from before 512 rows (power row 270 and feat row 200
/// known) loads too.
fn sheet_rows(old: bool) -> Result<String, String> {
    let mut sheet = caster();
    let versions = if old {
        sheet.powers.insert(270);
        sheet.feats.insert(200);
        crate::wire::Versions {
            snapshot: crate::since::snapshot::FEAT_POWER_ROWS.1 - 1,
            ..crate::wire::Versions::THIS_BUILD
        }
    } else {
        sheet.powers.insert(400);
        sheet.feats.insert(400);
        sheet.gear.feats.insert(450);
        crate::wire::Versions::THIS_BUILD
    };
    let read = sheet_bytes(&sheet, versions)?;
    check(
        read == sheet,
        format!(
            "powers {:?}, feats {:?}, item feats {:?} read back as {:?}, {:?}, {:?}",
            sheet.powers.rows().collect::<Vec<_>>(),
            sheet.feats.rows().collect::<Vec<_>>(),
            sheet.gear.feats.rows().collect::<Vec<_>>(),
            read.powers.rows().collect::<Vec<_>>(),
            read.feats.rows().collect::<Vec<_>>(),
            read.gear.feats.rows().collect::<Vec<_>>(),
        ),
    )
}

/// Force Aura and Resist Force, each cast through an impact script, run
/// together: KOTOR's scripts replace only powers of the cast power's own
/// tree. JA's rule that protect and absorb exclude each other is not
/// applied to them (the Jedi turns on Force Aura, then Resist Force).
fn buff_trees_coexist() -> Result<String, String> {
    use crate::combat::spells::{FORCE_POWER_FORCE_AURA, FORCE_POWER_RESIST_FORCE};
    let rows = [FORCE_POWER_FORCE_AURA, FORCE_POWER_RESIST_FORCE];
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let size = usize::from(FORCE_POWER_RESIST_FORCE) + 1;
    data.force_costs = vec![0; size];
    let mut spells = vec![Talent::default(); size];
    let mut impacts = vec![String::new(); size];
    for row in rows {
        data.force_costs[usize::from(row)] = COST;
        spells[usize::from(row)] = Talent {
            menu: Menu::Friendly,
            ..Talent::default()
        };
        impacts[usize::from(row)] = "probe_buff".into();
    }
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let library =
        crate::probe::library_with(CATALOG, [("probe_buff", script(&catalog, &[])?)], talents);
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = rows
        .iter()
        .fold(Rows::default(), |known, &row| known.with(row));
    jedi.flags |= flags::IMMORTAL;
    let config = WorldConfig {
        sheet: jedi,
        ..WorldConfig::one_seat()
    };
    let level = ground.with_module(data).with_scripts(library);
    let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
    let me = world.snapshot().body(Seat(0)).ok_or("no player")?.id;
    for row in rows {
        let press = Command {
            force: row,
            buttons: buttons::FORCE_POWER,
            ..Command::default()
        };
        step(&mut world, press);
        for _ in 0..4 {
            step(&mut world, press.idle());
        }
    }
    let at = now(&world);
    let force = fighter(&mut world, me).ok_or("no Jedi")?.force;
    check(
        force.protect_until > at && force.absorb_until > at,
        format!(
            "at {at} ms: Force Aura until {}, Resist Force until {}",
            force.protect_until, force.absorb_until
        ),
    )
}

pub fn probes() -> Checks {
    let shapes: [(&'static str, u16, [bool; 3]); 2] = [
        // Reach 3 m + 1: the foe aimed at and the one beside it in the
        // sphere, not the one outside it.
        ("power_rows_hostile_own_shape", 30, [true, true, false]),
        // Reach 0.5 m + 1: the foe 2.5 m off is out of range.
        ("power_rows_hostile_out_of_range", 5, [false; 3]),
    ];
    let mut checks: Checks = shapes
        .into_iter()
        .map(|(name, reach, hurt)| (name, hostile_shape(reach, hurt)))
        .collect();
    checks.push(("power_rows_minds_read_rows", minds_read_rows()));
    checks.extend(creature_casts());
    checks.push(("power_rows_force_routines_bound", force_routines_bound()));
    checks.push(("power_rows_buff_trees_coexist", buff_trees_coexist()));
    let sheets: [(&'static str, bool); 2] = [
        ("power_rows_sheet_row_400_round_trip", false),
        ("power_rows_old_sheet_loads", true),
    ];
    checks.extend(sheets.map(|(name, old)| (name, sheet_rows(old))));
    checks
}
