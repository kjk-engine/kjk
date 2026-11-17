//! The probes of Force Scream's ear covering (`kjk sim probes --only
//! "force scream"`; `ForceState::cover_ears`), on the proving ground, no
//! install. A TSL Jedi screams at one foe standing in front of it:
//! - a foe that fails its saving throw in the scream's impact script covers
//!   its ears for `ja::EARS_MS`, raising its hands (JA's
//!   `BOTH_SONICPAIN_START`) and then holding them there
//!   (`BOTH_SONICPAIN_HOLD`), and is free again after;
//! - a foe that makes the save does not cover its ears;
//! - a droid that fails the save never covers its ears;
//! - with no impact script, a foe that fails the native Will save covers its
//!   ears all the same;
//! - a second scream less than `ja::EARS_IMMUNE_MS` after the covering ended
//!   does not cover the ears again, and one after that time does.

use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;

use super::{Fighting, Id, Player, SimWorld};
use crate::combat::force::ja;
use crate::combat::sheet::{factions, flags};
use crate::combat::{Fighter, Sheet};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::motion::{Anim, MotionState};
use crate::probe::{Checks, check};
use crate::script::{Talent, Talents};
use crate::snapshot::{Kind, SimId};
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

/// The row screamed: Force Scream.
const ROW: u16 = crate::combat::spells::FORCE_POWER_FORCE_SCREAM;

/// The scripted save's DCs: one no foe makes, one every foe makes.
const FAILED: i32 = 100;
const MADE: i32 = -100;

/// Where the Jedi and its foe stand (metres, the ground's own axes): the
/// foe two metres ahead of the Jedi, who faces +X.
const JEDI_AT: (f32, f32) = (-6.0, 0.0);
const FOE_AT: (f32, f32) = (-4.0, 0.0);

/// The longest a scream's covering takes to show (ms).
const SHOWS_WITHIN_MS: u64 = 1000;

const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
object GetFirstObjectInShape();
// 3
int WillSave(object oCreature, int nDC);
"#;

/// An impact script that asks the first one caught for a Will save
/// against `dc`, its answer in custom token 1.
fn impact(dc: i32) -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(dc)))
        .op(Op::Action {
            routine: 2,
            args: 0,
        })
        .op(Op::Action {
            routine: 3,
            args: 2,
        })
        .op(Op::Action {
            routine: 1,
            args: 1,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Action {
            routine: 0,
            args: 2,
        })
        .op(Op::Retn);
    asm.build()
}

/// How the scream is decided: by an impact script asking a save against
/// its DC, or by the native rules with the foe's Will save made hopeless.
#[derive(Clone, Copy, Debug)]
enum Decided {
    Script(i32),
    Native,
}

/// The Jedi, its foe and the world they stand in.
struct Run {
    world: SimWorld,
    jedi: SimId,
    foe: SimId,
}

/// A point of the ground (metres) as a body's origin there (units).
fn origin(at: (f32, f32)) -> crate::math::Vec3 {
    crate::script::standing(crate::determinism::ground_point(at.0, at.1, 0.0))
}

/// What one scream did to the foe: when its covering began and ended (if
/// it covered its ears), and what its legs played at the start and once
/// the hands were up.
#[derive(Debug, Default)]
struct Covering {
    since: Option<u64>,
    until: u64,
    raising: Option<Anim>,
    held: Option<Anim>,
    free_after: bool,
}

impl Run {
    fn new(decided: Decided, droid: bool) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        data.game = "tsl".into();
        data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
        let size = usize::from(ROW) + 1;
        data.force_costs = vec![0; size];
        let mut foe = Sheet {
            faction: factions::HOSTILE_1,
            ..Sheet::dev_soldier()
        };
        if droid {
            foe.flags |= flags::DROID;
        }
        if matches!(decided, Decided::Native) {
            foe.save_bonus[2] = i8::MIN;
        }
        data.placements.push(Placement::Own(ObjectData {
            sheet: Some(foe),
            template: "foe".into(),
            ..ObjectData::bare(
                ObjectKind::Creature,
                "foe",
                crate::determinism::ground_point(FOE_AT.0, FOE_AT.1, 0.0),
            )
        }));
        let mut impacts = vec![String::new(); size];
        let mut scripts = Vec::new();
        if let Decided::Script(dc) = decided {
            impacts[usize::from(ROW)] = "probe_scream".into();
            scripts.push(("probe_scream", impact(dc)));
        }
        let talents = Talents {
            spells: vec![Talent::default(); size],
            impacts,
            ..Talents::default()
        };
        let library = crate::probe::library_with(CATALOG, scripts, talents);
        let level = ground.with_module(data).with_scripts(library);
        let mut jedi = Sheet::dev_jedi();
        jedi.powers.insert(ROW);
        jedi.flags |= flags::TSL | flags::IMMORTAL;
        let config = WorldConfig {
            sheet: jedi,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
        let start = world.snapshot();
        let jedi = start.body(Seat(0)).ok_or("no Jedi")?.id;
        let foe = start
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
            .ok_or("no foe")?
            .id;
        let mut bodies = world.world.query::<(&Id, &mut Player)>();
        for (id, mut player) in bodies.iter_mut(&mut world.world) {
            if id.0 == jedi {
                let at = origin(JEDI_AT);
                player.0.origin[0] = at[0];
                player.0.origin[1] = at[1];
            }
        }
        Ok(Run { world, jedi, foe })
    }

    fn step(&mut self, command: Command) {
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let tick = self.world.tick() + 1;
        crate::step(&mut self.world, tick, &input, MSEC, StepReason::Authority);
    }

    fn now(&self) -> u64 {
        self.world.world.resource::<super::Clock>().time_ms
    }

    fn fighter(&mut self, who: SimId) -> Option<Fighter> {
        let mut fighters = self.world.world.query::<(&Id, &Fighting)>();
        fighters
            .iter(&self.world.world)
            .find(|(id, _)| id.0 == who)
            .map(|(_, fighting)| fighting.0)
    }

    fn body(&mut self, who: SimId) -> Option<MotionState> {
        let mut bodies = self.world.world.query::<(&Id, &Player)>();
        bodies
            .iter(&self.world.world)
            .find(|(id, _)| id.0 == who)
            .map(|(_, player)| player.0)
    }

    /// `ms` of standing, the Jedi's pool kept full.
    fn wait(&mut self, ms: u64) {
        for _ in 0..ms / u64::from(MSEC) {
            self.step(Command::default());
        }
    }

    /// The Jedi screams, and the foe is watched until its covering ends
    /// (or `SHOWS_WITHIN_MS` if it never covers its ears).
    fn scream(&mut self) -> Covering {
        let jedi = self.jedi;
        let mut refill = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in refill.iter_mut(&mut self.world.world) {
            if id.0 == jedi {
                fighting.0.fp = 100 * 100;
            }
        }
        let press = Command {
            force: ROW,
            buttons: buttons::FORCE_POWER,
            ..Command::default()
        };
        self.step(press);
        let pressed = self.now();
        let raise = u64::try_from(
            self.world
                .level()
                .anims
                .get(Anim::BOTH_SONICPAIN_START)
                .length_ms(),
        )
        .unwrap_or(0);
        let mut seen = Covering::default();
        loop {
            self.step(press.idle());
            let now = self.now();
            let foe = self.foe;
            let (Some(fighter), Some(body)) = (self.fighter(foe), self.body(foe)) else {
                return seen;
            };
            match (fighter.force.covering_ears(now), seen.since) {
                // A covering a script's save began shows from the step
                // after (the hold arrives after the bodies moved).
                (Some(since), None) => {
                    seen.since = Some(since);
                    seen.until = fighter.force.stunned_until;
                }
                (Some(_), Some(_)) if seen.raising.is_none() => {
                    seen.raising = Some(body.legs.anim);
                }
                (Some(since), Some(_)) if seen.held.is_none() && now >= since + raise => {
                    seen.held = Some(body.legs.anim);
                }
                (None, Some(_)) => {
                    seen.free_after = !fighter.force.stunned(now);
                    return seen;
                }
                (None, None) if now > pressed + SHOWS_WITHIN_MS => return seen,
                _ => {}
            }
        }
    }
}

/// Whether `seen` is a whole covering: as long as the rulebook says, the
/// hands raised then held, free after.
fn covered_whole(seen: &Covering) -> bool {
    seen.since
        .is_some_and(|since| seen.until - since == ja::EARS_MS)
        && seen.raising == Some(Anim::BOTH_SONICPAIN_START)
        && seen.held == Some(Anim::BOTH_SONICPAIN_HOLD)
        && seen.free_after
}

/// One scream at a foe decided by `decided` (a droid if `droid`): whether
/// it covered its ears whole, or did not cover them at all, as `covers`
/// expects.
fn one_scream(decided: Decided, droid: bool, covers: bool) -> Result<String, String> {
    let mut run = Run::new(decided, droid)?;
    run.wait(200);
    let seen = run.scream();
    let right = if covers {
        covered_whole(&seen)
    } else {
        seen.since.is_none()
    };
    check(right, format!("{decided:?}, droid {droid}: {seen:?}"))
}

/// A scream inside the immunity after a covering does not cover the ears
/// again; one after it does.
fn immunity_window() -> Result<String, String> {
    let mut run = Run::new(Decided::Script(FAILED), false)?;
    run.wait(200);
    let first = run.scream();
    run.wait(ja::EARS_IMMUNE_MS / 4);
    let inside = run.scream();
    run.wait(ja::EARS_IMMUNE_MS);
    let after = run.scream();
    check(
        covered_whole(&first) && inside.since.is_none() && covered_whole(&after),
        format!("first {first:?}; inside the immunity {inside:?}; after it {after:?}"),
    )
}

pub fn probes() -> Checks {
    let ones: [(&'static str, Decided, bool, bool); 4] = [
        (
            "scream_failed_save_covers",
            Decided::Script(FAILED),
            false,
            true,
        ),
        (
            "scream_made_save_does_not",
            Decided::Script(MADE),
            false,
            false,
        ),
        (
            "scream_droid_never_covers",
            Decided::Script(FAILED),
            true,
            false,
        ),
        ("scream_native_save_covers", Decided::Native, false, true),
    ];
    let mut checks: Checks = ones
        .into_iter()
        .map(|(name, decided, droid, covers)| (name, one_scream(decided, droid, covers)))
        .collect();
    checks.push(("scream_immunity_window", immunity_window()));
    checks
}
