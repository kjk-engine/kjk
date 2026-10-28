//! `kjk combat force`: the Force's evidence without a window (I26). A dev
//! Jedi (`Sheet::dev_jedi`) stands 160 units from a hostile creature of a
//! module (end_m01aa's Sith soldier by default) and casts each power at it,
//! one scene per power from the same start, stepped on the authority and
//! recorded. Each scene prints what happened and checks it against the
//! rules (`sim::combat::force`, `docs/COMBAT.md`, the costs of
//! `spells.2da`); every recording replays to its end, and the push is also
//! stepped as a prediction, which must shove as the authority did. Writes
//! `facts.json`.

use std::process::ExitCode;

use sim::combat::damage;
use sim::combat::{Blow, FriendlyFire, Location, Relation, Source};
use sim::combat::{Sheet, mind};
use sim::combat::{effects, force};
use sim::motion;
use sim::{Command, Kind, Seat, SimRng, SimWorld, Snapshot, StepReason, TickInput, buttons, step};
use vfs::Discovery;

use crate::scenes::{Report, Stage, aim, fighter, hold, player, seat0};

/// The spells.2da cost of `row`, in hundredths.
fn cost(stage: &Stage, row: u16) -> i32 {
    stage
        .level
        .module()
        .and_then(|module| module.force_costs.get(usize::from(row)).copied())
        .map_or(0, |cost| i32::from(cost) * 100)
}

/// Presses the power button with `row` selected on the first step only.
fn tap(row: u8) -> impl FnMut(u64) -> (u16, u8, i8) {
    move |ms| {
        if ms == 0 {
            (buttons::FORCE_POWER, row, 0)
        } else {
            (0, row, 0)
        }
    }
}

/// The reference quirks of the Force fixed by the owner (QUIRKS.md Q-0031,
/// Q-0032, Q-0044; protect's Q-0033 is checked with protect): each one
/// where JA's way and ours part.
fn fixed_quirks(stage: &Stage, start: &Snapshot, target: usize, report: &mut Report) {
    let me = seat0(start);

    // Q-0031: a hit that may not harm (the party's, on a neutral outsider)
    // pushes nothing; a hostile's is shoved.
    outln!("  a refused hit's push (Q-0031)");
    let shove = |relation| {
        let blow = Blow {
            attacker: None,
            relation,
            source: Source::Power {
                points: 20,
                extra: 0,
                area: false,
                pushes: true,
                damage_type: None,
            },
            location: Location::None,
            direction: Some([1.0, 0.0, 0.0]),
            sneak: 0,
        };
        let mut rng = SimRng::new(1, 0);
        damage::assess(
            &blow,
            None,
            &Sheet::dev_jedi(),
            FriendlyFire::Off,
            sim::combat::passive::Game::K1,
            &mut rng,
        )
            .push
            .map(|(velocity, _)| velocity[0])
    };
    let (hostile, outsider) = (
        shove(Relation::Hostile),
        shove(Relation::Outsider { aimed: false }),
    );
    report.check(
        hostile.is_some() && outsider.is_none(),
        format!(
            "20 JA points that push: a hostile shoved at {hostile:?} u/s, the party's neutral outsider (no harm) at {outsider:?}"
        ),
    );

    // Q-0032: lightning's line leaves the eye. Aimed low from 100 units, a
    // line from the origin (the waist) meets the floor before the target.
    // The caster knows lightning up to level 2 (Force Lightning, not Storm):
    // the line, not level 3's cone.
    outln!("  lightning (Force Lightning, row 35) aimed at the shins from 100 units (Q-0032)");
    let mut liner = Sheet::dev_jedi();
    liner.powers.0[0] &= !(1 << 25);
    match stage.scene(target, 100.0, liner) {
        Some(close) => {
            let scene = stage.drive(&close, 1200, &mut |last, since| {
                let (Some(mine), Some(theirs)) =
                    (last.entities[me].player, last.entities[target].player)
                else {
                    return Command::default();
                };
                let [x, y, z] = theirs.origin;
                let shins = [x, y, z + motion::SOLE_Z + 16.0];
                let (pitch, yaw) = mind::aim(mind::eye(&mine), shins);
                Command {
                    buttons: if since < 1000 {
                        buttons::FORCE_LIGHTNING
                    } else {
                        0
                    },
                    ..mind::looking(&mine, pitch, yaw)
                }
            });
            let before = fighter(&close, target);
            let struck = scene.snapshots.iter().any(|snapshot| {
                let now = fighter(snapshot, target);
                now.next_event != before.next_event || now.hp < before.hp
            });
            let eye = player(&close, me).eye_height;
            report.check(
                struck,
                format!(
                    "the target {} (the eye, {eye:.0} units above the origin, sees it over the floor the origin's line meets)",
                    if struck { "struck" } else { "missed" }
                ),
            );
            report.replayed("lightning from the eye", scene.replayed);
        }
        None => report.check(false, "no clear spot 100 units away".to_string()),
    }

    // Q-0044: a raging character can be killed: the Jedi's saber on a
    // target raging at level 1 with 1 HP left (its rage's own cost held
    // off). K1's lightning hurts by its impact script, which rage leaves be.
    outln!("  the saber on a target raging at level 1, 1 HP left (Q-0044)");
    let Some(mut raging) = stage.scene(target, 40.0, Sheet::dev_jedi()) else {
        report.check(false, "no clear spot 40 units away".to_string());
        return;
    };
    if let Some(theirs) = raging.entities[target].fighter.as_mut() {
        theirs.hp = 100;
        theirs.force.rage_level = 1;
        theirs.force.rage_until = raging.time_ms + 60_000;
        theirs.force.rage_paid_at = raging.time_ms + 60_000;
    }
    let scene = stage.run(&raging, target, 4000, &mut hold(buttons::ATTACK, 4000));
    let my_id = raging.entities[me].id;
    let last = fighter(scene.snapshots.last().expect("stepped"), target);
    let blows: Vec<f64> = last
        .events
        .iter()
        .flatten()
        .filter(|event| event.by == Some(my_id) && event.tick > raging.tick)
        .map(|event| f64::from(event.damage) / 100.0)
        .collect();
    report.check(
        !last.alive() && !blows.is_empty(),
        format!(
            "{} at {:.2} HP after 4 s of swings (blows landed for {blows:?} HP)",
            if last.alive() { "alive" } else { "killed" },
            f64::from(last.hp) / 100.0
        ),
    );
    report.replayed("raging target", scene.replayed);
}

/// What a scene did to the caster and its target, over all its steps: the
/// least and most of each pool and what held or moved them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Marks {
    target_hp: i32,
    target_lasting: usize,
    target_least: usize,
    target_held: bool,
    target_confused: bool,
    target_slowed: bool,
    target_down: bool,
    target_shoved: bool,
    caster_hp: i32,
    caster_lasting: usize,
    caster_toggle: bool,
    caster_blessed: bool,
}

impl Marks {
    fn of(run: &crate::scenes::Run, me: usize, target: usize) -> Marks {
        let mut marks = Marks {
            target_hp: i32::MAX,
            target_least: usize::MAX,
            ..Marks::default()
        };
        for snapshot in &run.snapshots {
            let now = snapshot.time_ms;
            let (mine, theirs) = (fighter(snapshot, me), fighter(snapshot, target));
            let lasting = |f: &sim::combat::Fighter| f.effects.lasting.iter().flatten().count();
            let force = theirs.force;
            let velocity = player(snapshot, target).velocity;
            marks.target_hp = marks.target_hp.min(theirs.hp);
            marks.target_lasting = marks.target_lasting.max(lasting(&theirs));
            marks.target_least = marks.target_least.min(lasting(&theirs));
            marks.target_held |= force.stunned(now) || force.gripped_by.is_some();
            marks.target_confused |= force.confused_until > now;
            marks.target_slowed |= theirs.effects.slowed.1 > now || theirs.effects.sapped.1 > now;
            marks.target_down |= theirs.saber.floored();
            marks.target_shoved |= velocity[0].hypot(velocity[1]) > 400.0;
            marks.caster_hp = marks.caster_hp.max(mine.hp);
            marks.caster_lasting = marks.caster_lasting.max(lasting(&mine));
            let own = mine.force;
            marks.caster_toggle |= [
                own.speed_until,
                own.protect_until,
                own.absorb_until,
                own.trick_until,
                own.rage_until,
                own.sight_until,
            ]
            .iter()
            .any(|until| *until > now);
            marks.caster_blessed |= mine.effects.valor(now) > 0 || mine.effects.resisting(now) > 0;
        }
        marks
    }

    /// What differs from `baseline`, in words.
    fn against(&self, baseline: &Marks) -> Vec<String> {
        let mut seen = Vec::new();
        let mut note = |differs: bool, what: String| {
            if differs {
                seen.push(what);
            }
        };
        note(
            self.target_hp < baseline.target_hp,
            format!(
                "target hurt {:.0} HP",
                f64::from(baseline.target_hp - self.target_hp) / 100.0
            ),
        );
        note(
            self.target_lasting > baseline.target_lasting,
            format!("target effects +{}", self.target_lasting - baseline.target_lasting),
        );
        note(
            self.target_least < baseline.target_least,
            format!("target effects −{}", baseline.target_least - self.target_least),
        );
        let flags = [
            (self.target_held && !baseline.target_held, "target held"),
            (self.target_confused && !baseline.target_confused, "target confused"),
            (self.target_slowed && !baseline.target_slowed, "target slowed"),
            (self.target_down && !baseline.target_down, "target knocked down"),
            (self.target_shoved && !baseline.target_shoved, "target shoved"),
            (self.caster_toggle && !baseline.caster_toggle, "caster's power running"),
            (self.caster_blessed && !baseline.caster_blessed, "caster blessed"),
        ];
        for (differs, what) in flags {
            note(differs, what.to_string());
        }
        note(
            self.caster_hp > baseline.caster_hp,
            format!(
                "caster healed {:.0} HP",
                f64::from(self.caster_hp - baseline.caster_hp) / 100.0
            ),
        );
        note(
            self.caster_lasting > baseline.caster_lasting,
            format!("caster effects +{}", self.caster_lasting - baseline.caster_lasting),
        );
        seen
    }
}

/// Every K1 power cast alone at the target (a droid for the droid powers,
/// under Burst of Speed for the dispels; failing every save) by an
/// immortal Jedi at 10 HP who knows only it: each must pay its cost and do
/// something the same scene without it does not.
fn every_power(stage: &Stage, target: usize, report: &mut Report) {
    use sim::combat::sheet::{flags, races};
    use sim::combat::spells::*;
    outln!("  every K1 power alone, 2.5 s each, against a scene without it");
    let objects = sim::Instances::in_snapshot(&stage.level, &stage.start);
    let weak = Sheet {
        saves: [-20; 3],
        ..crate::cast::sheet(&stage.start.entities[target], &objects)
    };
    let stages = [
        stage.with_creature_sheet(target, weak),
        stage.with_creature_sheet(
            target,
            Sheet {
                race: races::DROID,
                flags: weak.flags | flags::DROID,
                ..weak
            },
        ),
    ];
    let caster = |row: Option<u16>| {
        let mut sheet = Sheet::dev_jedi();
        sheet.powers = row
            .into_iter()
            .fold(sim::combat::sheet::Rows::default(), |known, row| known.with(row));
        sheet.flags |= flags::IMMORTAL;
        sheet
    };
    // (target a droid, target buffed): the scene for `row`.
    let scene = |(droid, buffed): (bool, bool), row: Option<u16>| {
        let stage = stages[usize::from(droid)].as_ref()?;
        let mut start = stage.scene(target, 160.0, caster(row))?;
        let me = seat0(&start);
        if let Some(mine) = start.entities[me].fighter.as_mut() {
            mine.hp = 1000;
        }
        let time = start.time_ms;
        let own = start.entities[target].id.0;
        if buffed && let Some(theirs) = start.entities[target].fighter.as_mut() {
            // Burst of Speed as its script leaves it, for either dispel to
            // end (Supress Force ends the first two tiers only).
            (theirs.force.speed_until, theirs.force.speed_level) = (time + 60_000, 1);
            let lasting = effects::Lasting {
                modifies: effects::Modifies::Defence,
                amount: 4,
                of: 0,
                until: time + 60_000,
                spell: FORCE_POWER_SPEED_BURST,
                by: own,
                id: 1,
                at: time,
            };
            theirs.effects.add(lasting, time);
        }
        let throw = matches!(
            row,
            Some(FORCE_POWER_LIGHT_SABER_THROW | FORCE_POWER_LIGHT_SABER_THROW_ADVANCED)
        );
        let selected = row.and_then(|row| u8::try_from(row).ok()).unwrap_or(0);
        // The first alternate lights the saber; held on past the draw, it
        // throws.
        let (button, for_ms) = if throw {
            (buttons::ALT_ATTACK, 1500)
        } else {
            (buttons::FORCE_POWER, 1000)
        };
        let run = stage.run(&start, target, 2500, &mut |ms| {
            let held = if row.is_some() && ms < for_ms { button } else { 0 };
            (held, selected, 0)
        });
        let paid = start.entities[me].fighter.map_or(0, |f| f.fp)
            - run.snapshots.iter().map(|s| fighter(s, me).fp).min().unwrap_or(0);
        Some((
            Marks::of(&run, me, target),
            paid.max(0),
            run.replayed,
            stage,
        ))
    };
    let kinds = [(false, false), (true, false), (false, true)];
    let baselines = kinds.map(|kind| scene(kind, None).map(|(marks, ..)| marks));
    let mut failed = Vec::new();
    for row in K1_FORCE_POWERS {
        let kind = (
            force::crowd::droids(row),
            matches!(row, FORCE_POWER_FORCE_BREACH | FORCE_POWER_SUPRESS_FORCE),
        );
        let baseline = kinds
            .iter()
            .position(|k| *k == kind)
            .and_then(|at| baselines[at].as_ref());
        let (Some(baseline), Some((marks, paid, replayed, on))) = (baseline, scene(kind, Some(row)))
        else {
            failed.push(row);
            outln!("    {row:>3}: no scene");
            continue;
        };
        let seen = marks.against(baseline);
        let cost = cost(on, row);
        let works = (paid >= cost || cost == 0) && !seen.is_empty() && replayed;
        if !works {
            failed.push(row);
        }
        outln!(
            "    {} {row:>3} {:<40} paid {:>3} FP (costs {:>2}): {}",
            if works { "ok " } else { "BAD" },
            label(row),
            paid / 100,
            cost / 100,
            if seen.is_empty() {
                "nothing seen".to_string()
            } else {
                seen.join(", ")
            }
        );
    }
    let all = K1_FORCE_POWERS.len();
    report.check(
        failed.is_empty(),
        format!(
            "{} of {all} K1 powers do something; not: {failed:?}",
            all - failed.len()
        ),
    );
}

/// A `spells.2da` row's label where `combat::spells` names it.
fn label(row: u16) -> &'static str {
    sim::combat::spells::NAMED
        .iter()
        .find(|(_, named)| *named == row)
        .map_or("?", |(label, _)| label)
}

/// `kjk combat force`.
pub fn force(
    discovery: &Discovery,
    game: &str,
    module: &str,
    target_tag: &str,
    drained_tag: &str,
) -> ExitCode {
    let hostile = [target_tag.to_string(), drained_tag.to_string()];
    let stage = match Stage::new(discovery, game, module, &hostile) {
        Ok(stage) => stage,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let (Some(target), Some(drained)) = (stage.creature(target_tag), stage.creature(drained_tag))
    else {
        eprintln!("no creature tagged {target_tag} or {drained_tag} in {module}");
        return ExitCode::FAILURE;
    };
    let me = seat0(&stage.start);
    let objects = sim::Instances::in_snapshot(&stage.level, &stage.start);
    let sheet_of = |at: usize| {
        let entity = &stage.start.entities[at];
        match entity.kind {
            Kind::Object(..) => crate::cast::sheet(entity, &objects),
            _ => Sheet::dev_jedi(),
        }
    };
    let target_sheet = sheet_of(target);
    outln!(
        "combat force {module}: the dev Jedi (level 6, 54 HP, 60 FP) against {target_tag} ({} HP, flags {}) and {drained_tag} ({} FP)",
        target_sheet.max_hp,
        target_sheet.flags,
        sheet_of(drained).max_fp
    );
    let mut report = Report::default();
    let Some(start) = stage.scene(target, 160.0, Sheet::dev_jedi()) else {
        eprintln!("no clear spot near {target_tag}");
        return ExitCode::FAILURE;
    };
    // Regeneration: an empty pool refills in 20 s (C13).
    outln!("  regeneration");
    let mut empty = start.clone();
    if let Some(fighter) = empty.entities[me].fighter.as_mut() {
        fighter.fp = 0;
    }
    let scene = stage.run(&empty, target, 21_000, &mut |_| (0, 0, 0));
    let full = scene
        .snapshots
        .iter()
        .find(|snapshot| fighter(snapshot, me).fp >= 6000)
        .map(|snapshot| snapshot.time_ms - start.time_ms);
    let every = force::regen_every(60);
    report.check(
        full.is_some_and(|ms| ms.abs_diff(20_000) <= every + 8),
        format!("0 to 60 FP in {full:?} ms (C13: the pool in 20 s, a point every {every} ms)"),
    );
    report.replayed("regeneration", scene.replayed);

    // Speed: Master Speed, × 1.7 for 20 s.
    outln!("  speed (Master Speed, row 37)");
    let scene = stage.run(&start, target, 200, &mut tap(37));
    let after = &scene.snapshots[0];
    let state = fighter(after, me);
    report.check(
        player(after, me).run_speed == motion::BASE_SPEED * force::ja::SPEED_SCALE,
        format!("base speed {} (250 × 1.7)", player(after, me).run_speed),
    );
    report.check(
        state.fp == 6000 - cost(&stage, 37) && state.force.speed_until == after.time_ms + 20_000,
        format!(
            "paid {} FP (spells.2da: {}); runs {} ms",
            (6000 - state.fp) / 100,
            cost(&stage, 37) / 100,
            state.force.speed_until - after.time_ms
        ),
    );
    report.replayed("speed", scene.replayed);

    // Push and pull at level 3 (Force Wave): the shove's speed.
    for (name, row, pull) in [
        ("push (Force Wave, row 26)", 26u8, false),
        ("pull (JA's, known with push)", force::PULL, true),
    ] {
        outln!("  {name}");
        let scene = stage.run(&start, target, 100, &mut tap(row));
        let after = &scene.snapshots[0];
        let (mine, theirs) = (player(&start, me), player(&start, target));
        let mut away = sim::math::sub(theirs.origin, mine.origin);
        let distance = sim::math::normalize(&mut away);
        let expected = (768.0 - 0.7 * distance).max(16.0);
        let moved = player(after, target).velocity;
        let flat = (moved[0] * moved[0] + moved[1] * moved[1]).sqrt();
        let toward = sim::math::dot([moved[0], moved[1], 0.0], away);
        report.check((flat - expected * (away[0] * away[0] + away[1] * away[1]).sqrt()).abs() <= 4.0
                && (toward < 0.0) == pull,
            format!(
                "{} at {flat:.0} u/s level (768 − 0.7 × {distance:.0} = {expected:.0}), {:.0} u/s up",
                if pull { "pulled in" } else { "pushed away" },
                moved[2]
            ),
        );
        report.check(
            fighter(after, me).fp == 6000 - cost(&stage, 26),
            format!(
                "paid {} FP (Force Wave's {})",
                (6000 - fighter(after, me).fp) / 100,
                cost(&stage, 26) / 100
            ),
        );
        report.replayed(if pull { "pull" } else { "push" }, scene.replayed);
        if !pull {
            // The same step predicted: the shove and the cost are the same.
            let mut world =
                SimWorld::from_snapshot(&start, stage.level.clone()).expect("the scene starts");
            let mut input = TickInput::default();
            input.insert(
                Seat(0),
                aim(&start, me, target, (buttons::FORCE_POWER, row, 0)),
            );
            let tick = start.tick + 1;
            let predicted = step(
                &mut world,
                tick,
                &input,
                stage.rate.msec(tick),
                StepReason::Predict,
            );
            report.check(
                player(&predicted, target).velocity == moved
                    && fighter(&predicted, me).fp == fighter(after, me).fp,
                "predicted: the same shove and cost as the authority".to_string(),
            );
        }
    }

    // A wave close up knocks down (I23 5.2: level 3 against level 0 within
    // 128 units); the one from 160 above did not.
    outln!("  push close up (Force Wave from 100 units)");
    let pushed_far = stage.run(&start, target, 100, &mut tap(26));
    let far = fighter(&pushed_far.snapshots[0], target)
        .saber
        .floored();
    match stage.scene(target, 100.0, Sheet::dev_jedi()) {
        Some(close) => {
            let scene = stage.run(&close, target, 100, &mut tap(26));
            let saber = fighter(&scene.snapshots[0], target).saber;
            report.check(saber.floored() && saber.quicker && !far,
                format!(
                    "knocked down from 100 units ({}), with the quicker getup ({}); not from 160 ({})",
                    saber.floored(),
                    saber.quicker,
                    far
                ),
            );
            report.replayed("knockdown", scene.replayed);
        }
        None => report.check(false, "no clear spot 100 units away".to_string()),
    }

    // Absorb against the push: level 2 absorb leaves the wave level 1.
    outln!("  push against absorb 2");
    let mut absorbing = start.clone();
    if let Some(fighter) = absorbing.entities[target].fighter.as_mut() {
        fighter.force.absorb_until = start.time_ms + 20_000;
        fighter.force.absorb_level = 2;
    }
    let scene = stage.run(&absorbing, target, 100, &mut tap(26));
    let moved = player(&scene.snapshots[0], target).velocity;
    let distance = sim::math::length(sim::math::sub(
        player(&start, target).origin,
        player(&start, me).origin,
    ));
    let flat = (moved[0] * moved[0] + moved[1] * moved[1]).sqrt();
    report.check(
        flat <= (256.0 - 0.7 * distance).max(16.0) + 2.0,
        format!("shoved at {flat:.0} u/s (level 1: 256 − 0.7 × {distance:.0})"),
    );
    report.replayed("absorbed push", scene.replayed);

    // Grip at level 3 (Kill): held until it lets go.
    outln!("  grip (Kill, row 32), held 5 s");
    let scene = stage.run(&start, target, 5000, &mut hold(buttons::FORCE_GRIP, 5000));
    let latched = scene
        .snapshots
        .iter()
        .position(|snapshot| fighter(snapshot, target).force.gripped_by.is_some());
    let let_go = latched.and_then(|from| {
        scene.snapshots[from..]
            .iter()
            .position(|snapshot| fighter(snapshot, target).force.gripped_by.is_none())
            .map(|len| from + len)
    });
    let at_ms = |at: usize| scene.snapshots[at].time_ms - start.time_ms;
    let crush = u64::from(force::ja::GRIP_CRUSH[2] as u16) * u64::from(target_sheet.max_hp);
    let crushed = scene.snapshots.iter().find_map(|snapshot| {
        fighter(snapshot, target)
            .newest()
            .find(|event| event.damage as u64 == crush)
            .map(|event| event.tick)
    });
    outln!(
        "    latched at {:?} ms, let go at {:?} ms; the crush ({crush} hundredths: 40% of {} HP) at tick {crushed:?}; {} HP left; {} FP left",
        latched.map(at_ms),
        let_go.map(at_ms),
        target_sheet.max_hp,
        f64::from(scene.snapshots.last().map_or(0, |s| fighter(s, target).hp)) / 100.0,
        f64::from(scene.snapshots.last().map_or(0, |s| fighter(s, me).fp)) / 100.0,
    );
    let lifted = latched.is_some_and(|at| player(&scene.snapshots[at + 1], target).gravity == 0.0);
    report.check(
        latched == Some(0) && lifted,
        "latched at once; the victim floats (no gravity at level 3)".to_string(),
    );
    // Kill's impact is its script's (`k_sp1_generic`, C10): a sixth of the
    // victim's most HP a second, three times, from the latch.
    let hurt_by = latched.and_then(|at| {
        let from = fighter(&scene.snapshots[at], target).hp;
        scene.snapshots[at..]
            .iter()
            .take(438)
            .find(|s| fighter(s, target).hp < from || !fighter(s, target).alive())
            .map(|s| s.time_ms - scene.snapshots[at].time_ms)
    });
    report.check(
        hurt_by.is_some(),
        format!("Kill's impact script hurts the victim within 3.5 s of the latch ({hurt_by:?} ms)"),
    );
    // Held past the pool: 15 FP up front, 15 a second after the first half
    // second, so 60 FP last 3.5 s; or the 4 s limit.
    report.check(
        let_go.is_some_and(|at| at_ms(at) <= 4016),
        format!(
            "let go by 4 s ({:?} ms: the pool or the level-3 limit)",
            let_go.map(at_ms)
        ),
    );
    let after = scene.snapshots.last().expect("stepped");
    report.check(
        player(after, target).gravity == motion::GRAVITY
            && fighter(after, me).force.grip_ready > start.time_ms,
        "released: gravity back, the grip's 3 s wait running".to_string(),
    );
    report.replayed("grip", scene.replayed);

    // The grip is the damage: Kill's script ticks from code its impact
    // deferred (its iterative damage, 1, 3 and 5 s in), and the grip let go
    // takes those still to come with it.
    outln!("  grip (Kill) let go after 1.5 s");
    let scene = stage.run(&start, target, 6000, &mut hold(buttons::FORCE_GRIP, 1500));
    let hp_at = |ms: u64| {
        scene
            .snapshots
            .iter()
            .find(|snapshot| snapshot.time_ms - start.time_ms >= ms)
            .map(|snapshot| fighter(snapshot, target).hp)
    };
    let (before, released, end) = (
        fighter(&start, target).hp,
        hp_at(1600),
        scene.snapshots.last().map(|s| fighter(s, target).hp),
    );
    let held = scene
        .snapshots
        .iter()
        .rfind(|snapshot| fighter(snapshot, target).force.gripped_by.is_some())
        .map(|snapshot| snapshot.time_ms - start.time_ms);
    report.check(
        released.is_some_and(|hp| hp < before) && end == released && held <= Some(1600),
        format!("HP {before} → {released:?} at the release → {end:?} 6 s in; gripped to {held:?} ms"),
    );
    report.replayed("grip let go", scene.replayed);

    // Lightning at level 3 (Force Storm), held a second.
    outln!("  lightning (Force Storm, row 25), held 1 s");
    let scene = stage.run(
        &start,
        target,
        1000,
        &mut hold(buttons::FORCE_LIGHTNING, 1000),
    );
    let first = fighter(&start, target);
    let last = scene.snapshots.last().expect("stepped");
    let struck = fighter(last, target)
        .next_event
        .wrapping_sub(first.next_event);
    report.check(
        (19..=22).contains(&struck) || !fighter(last, target).alive(),
        format!(
            "{struck} strikes (one per 50 ms), {:.2} HP taken of {}",
            f64::from(first.hp - fighter(last, target).hp) / 100.0,
            target_sheet.max_hp
        ),
    );
    let paid = 6000 - fighter(last, me).fp;
    report.check(
        (paid - 3000).abs() <= 200,
        format!(
            "paid {:.2} FP (C12: 20 for the first half second, then 20 a second)",
            f64::from(paid) / 100.0
        ),
    );
    report.replayed("lightning", scene.replayed);

    // Drain at level 3 (Death Field) on a Force user, the caster hurt.
    outln!("  drain (Death Field, row 11) on {drained_tag}, the caster at 10 HP");
    if let Some(start) = stage.scene(drained, 160.0, Sheet::dev_jedi()) {
        let mut hurt = start.clone();
        if let Some(fighter) = hurt.entities[me].fighter.as_mut() {
            fighter.hp = 1000;
        }
        let scene = stage.run(&hurt, drained, 1000, &mut hold(buttons::FORCE_DRAIN, 1000));
        let last = scene.snapshots.last().expect("stepped");
        let theirs = sheet_of(drained);
        let taken = fighter(&hurt, drained).fp - fighter(last, drained).fp;
        let healed = fighter(last, me).hp - 1000;
        // Each tick takes 4 JA points of its pool (0.96 FP of 24) and heals
        // 4 of the caster's (2.16 HP of 54), 20 ticks a second.
        // Death Field's impact is its script's (C10): level d4 to the
        // target, as much healed to the caster.
        let _ = theirs;
        let lost = fighter(&hurt, drained).hp - fighter(last, drained).hp;
        report.check(
            (lost > 0 || !fighter(last, drained).alive()) && healed > 0,
            format!(
                "the target lost {:.2} HP, the caster healed {:.2} (Death Field's script; {:.2} FP of JA's drain taken)",
                f64::from(lost) / 100.0,
                f64::from(healed) / 100.0,
                f64::from(taken) / 100.0
            ),
        );
        report.replayed("drain", scene.replayed);
    } else {
        report.check(false, format!("no clear spot near {drained_tag}"));
    }

    // Heal (KOTOR's Heal): alone and hurt; its script heals (C10).
    outln!("  heal (Heal, row 28), the caster at 10 HP");
    let mut hurt = start.clone();
    if let Some(fighter) = hurt.entities[me].fighter.as_mut() {
        fighter.hp = 1000;
    }
    let scene = stage.run(&hurt, target, 100, &mut tap(28));
    let after = fighter(scene.snapshots.last().expect("stepped"), me);
    report.check(
        after.hp > 1000 && after.fp == 6000 - cost(&stage, 28),
        format!(
            "{:.2} HP (10 + Heal's script's), paid {} FP",
            f64::from(after.hp) / 100.0,
            (6000 - after.fp) / 100
        ),
    );
    report.replayed("heal", scene.replayed);

    // Protect: JA's worked example, and the cast.
    outln!("  protect");
    let example = force::protect(2, 5000, 100, 10_000, 100);
    report.check(
        example == (3000, 2500),
        format!("a 50-point hit on level 2 (JA's example): {example:?} (30 prevented, 25 FP paid)"),
    );
    let capped = force::protect(1, 15_000, 100, 10_000, 100);
    report.check(
        capped == (4000, 10_000),
        format!("150 points on level 1 (cap 100): {capped:?} (40 prevented, the 50 over the cap let through)"),
    );
    let scene = stage.run(&start, target, 100, &mut tap(17));
    let after = fighter(&scene.snapshots[0], me);
    report.check(
        after.force.protect_level == 3 && after.fp == 6000 - cost(&stage, 17),
        format!(
            "Force Armor: level {}, paid {} FP",
            after.force.protect_level,
            (6000 - after.fp) / 100
        ),
    );
    report.replayed("protect", scene.replayed);
    fixed_quirks(&stage, &start, target, &mut report);

    // Stun: cast until one lands (a Will save), then the cap and immunity.
    outln!("  stun (row 46): cast every 1.1 s until it lands");
    let scene = stage.run(&start, target, 4000, &mut |ms| {
        if ms % 1104 == 0 {
            (buttons::FORCE_POWER, 46, 0)
        } else {
            (0, 46, 0)
        }
    });
    let landed = scene
        .snapshots
        .iter()
        .find(|snapshot| fighter(snapshot, target).force.stunned_until > 0);
    // A save spares the stun; Stun's script then slows it instead.
    let slowed = scene.snapshots.iter().any(|snapshot| {
        fighter(snapshot, target)
            .effects
            .running(snapshot.time_ms)
            .any(|l| l.modifies == effects::Modifies::Speed && l.amount < 0)
    });
    match landed {
        Some(snapshot) => {
            let theirs = fighter(snapshot, target).force;
            report.check(
                theirs.stunned_until == snapshot.time_ms + 3000
                    && theirs.immune_until == snapshot.time_ms + 4000,
                format!(
                    "stunned at {} ms for 3 s, immune a second after (C11)",
                    snapshot.time_ms - start.time_ms
                ),
            );
            let later = scene
                .snapshots
                .iter()
                .find(|s| s.time_ms > snapshot.time_ms && s.time_ms < snapshot.time_ms + 3000);
            report.check(
                later.is_some_and(|s| fighter(s, target).last_command.buttons == 0),
                "the stunned presses nothing".to_string(),
            );
        }
        None => report.check(
            slowed,
            "no stun in 4 s (three casts): every save made, each one's lesser effect (slowed) landed".to_string(),
        ),
    }
    report.replayed("stun", scene.replayed);

    // KOTOR's guards before the save: the Sentinel's Force Immunity: Stun,
    // and Force Resistance (Force Immunity's 15 + level 20 = 35 against
    // d20 + 6: never reached). Three casts each, none may land.
    let guarded = |sheet: Sheet, absorbing: bool| {
        let stage = stage.with_creature_sheet(target, sheet)?;
        let mut start = stage.scene(target, 160.0, Sheet::dev_jedi())?;
        if absorbing && let Some(fighter) = start.entities[target].fighter.as_mut() {
            fighter.force.absorb_until = start.time_ms + 20_000;
            fighter.force.absorb_level = 2;
        }
        let scene = stage.run(&start, target, 4000, &mut |ms| {
            let pressed = if ms % 1104 == 0 {
                buttons::FORCE_POWER
            } else {
                0
            };
            (pressed, 46, 0)
        });
        let stunned = scene
            .snapshots
            .iter()
            .any(|snapshot| fighter(snapshot, target).force.stunned_until > 0);
        // A cast is a drop in the caster's pool.
        let casts = scene
            .snapshots
            .windows(2)
            .filter(|pair| fighter(&pair[1], me).fp < fighter(&pair[0], me).fp)
            .count()
            + usize::from(fighter(&scene.snapshots[0], me).fp < fighter(&start, me).fp);
        Some((stunned, casts, scene.replayed))
    };
    let immune = Sheet {
        feats: target_sheet
            .feats
            .with(sim::combat::sheet::feats::FORCE_IMMUNITY_STUN),
        ..target_sheet
    };
    let resisting = Sheet {
        level: 20,
        ..target_sheet
    };
    for (name, sheet, absorbing) in [
        ("Force Immunity: Stun (feat 99)", immune, false),
        ("Force Immunity (row 20) at level 20", resisting, true),
    ] {
        outln!("  stun (row 46) at a {target_tag} with {name}");
        match guarded(sheet, absorbing) {
            Some((stunned, casts, replayed)) => {
                report.check(
                    !stunned && casts == 3,
                    format!(
                        "{casts} casts, none landed: {}",
                        if stunned { "stunned" } else { "never stunned" }
                    ),
                );
                report.replayed("stun guarded", replayed);
            }
            None => report.check(false, format!("no scene for {name}")),
        }
    }

    // Mind trick (Dominate) on a creature that is no Force user.
    outln!("  mind trick (Dominate, row 14)");
    let scene = stage.run(&start, target, 500, &mut tap(14));
    let after = &scene.snapshots[0];
    let theirs = fighter(after, target);
    report.check(
        theirs.force.confused_until == after.time_ms + 10_000
            && scene
                .snapshots
                .iter()
                .skip(1)
                .all(|s| fighter(s, target).target.is_none()),
        format!(
            "confused for {} ms (level 2: 10 s), no enemy meanwhile",
            theirs.force.confused_until.saturating_sub(after.time_ms)
        ),
    );
    report.replayed("mind trick", scene.replayed);

    // KOTOR's effects (`sim::combat::effects`): what KOTOR's powers add to
    // JA's toggles, and its own effect powers.
    outln!(
        "  KOTOR's effects: Force Armor and Master Speed, then Master Valor and Improved Energy Resistance"
    );
    let now_of = |scene: &crate::scenes::Run| scene.snapshots.last().expect("stepped").time_ms;
    let jedi = Sheet::dev_jedi();
    let bonus = |scene: &crate::scenes::Run| {
        let state = fighter(scene.snapshots.last().expect("stepped"), me);
        (state, effects::sheet_now(jedi, &state, now_of(scene)))
    };
    // The second cast once the first's hand is put away.
    let casts = |rows: [u8; 2]| {
        move |ms: u64| match ms {
            0 => (buttons::FORCE_POWER, rows[0], 0),
            1104 => (buttons::FORCE_POWER, rows[1], 0),
            _ => (0, 0, 0),
        }
    };
    let scene = stage.run(&start, target, 1400, &mut casts([17, 37]));
    let (state, now) = bonus(&scene);
    report.check(
        now.defence() == jedi.defence() + 6 + 4
            && now.saves == jedi.saves.map(|save| save + 6)
            && effects::extra_attacks(&state.force, &state.effects, now_of(&scene)) == 2,
        format!(
            "Force Armor +6 defence and saves, Master Speed +4 defence and 2 attacks a round: defence {} → {}",
            jedi.defence(),
            now.defence()
        ),
    );
    report.replayed("armor and speed", scene.replayed);
    let scene = stage.run(&start, target, 1400, &mut casts([36, 40]));
    let (state, now) = bonus(&scene);
    report.check(
        now.abilities[0] == jedi.abilities[0] + 5
            && now.saves == jedi.saves.map(|save| save + 5)
            && now.guards.resist[7] == effects::RESIST
            && state.effects.poison_immune(now_of(&scene)),
        format!(
            "Master Valor: STR {} → {}, saves {:?} → {:?}; Improved Energy Resistance: resist {} electrical (level {}); immune to poison {}; {} FP left",
            jedi.abilities[0],
            now.abilities[0],
            jedi.saves,
            now.saves,
            now.guards.resist[7],
            state.effects.resisting(now_of(&scene)),
            state.effects.poison_immune(now_of(&scene)),
            state.fp / 100
        ),
    );
    report.replayed("valor and resistance", scene.replayed);

    outln!("  Plague (row 38) at {target_tag}");
    let scene = stage.run(&start, target, 13_500, &mut tap(38));
    let at = |ms: u64| &scene.snapshots[(ms / 8) as usize - 1];
    // Its impact script's: the virulent poison (`poison.2da` row 2, DC 100:
    // no save), a point of each ability a dose every 6 s, linked with half
    // speed.
    let drained = |ms: u64| fighter(at(ms), target).pack.drained(at(ms).time_ms);
    let (sick, sicker) = (drained(6400), drained(13_400));
    report.check(
        fighter(at(6400), target).pack.poison.is_some()
            && sick == [1; 6]
            && sicker == [2; 6]
            && player(at(6400), target).run_speed <= motion::BASE_SPEED * 0.5,
        format!(
            "poisoned and slowed (base speed {}); drained {sick:?} at 6.4 s, {sicker:?} at 13.4 s",
            player(at(6400), target).run_speed,
        ),
    );
    report.replayed("plague", scene.replayed);

    outln!("  Force Breach (row 19) at {target_tag} under speed, protect and valor");
    // The buffs as their scripts leave them: the toggles running, and
    // each power's lasting effect carrying its row (Master Speed 37, Force
    // Armor 17, Master Valor 36), for Breach's script to find.
    let mut buffed = start.clone();
    let time = buffed.time_ms;
    let own = buffed.entities[target].id.0;
    if let Some(theirs) = buffed.entities[target].fighter.as_mut() {
        (theirs.force.speed_until, theirs.force.speed_level) = (time + 60_000, 3);
        (theirs.force.protect_until, theirs.force.protect_level) = (time + 60_000, 3);
        for (id, (spell, modifies)) in [
            (37, effects::Modifies::Defence),
            (17, effects::Modifies::Defence),
            (36, effects::Modifies::Ability),
        ]
        .into_iter()
        .enumerate()
        {
            let lasting = effects::Lasting {
                modifies,
                amount: 4,
                of: 0,
                until: time + 60_000,
                spell,
                by: own,
                id: id as u32 + 1,
                at: time,
            };
            theirs.effects.add(lasting, time);
        }
    }
    let scene = stage.run(&buffed, target, 300, &mut tap(19));
    let after = scene.snapshots.last().expect("stepped");
    let theirs = fighter(after, target);
    let left = theirs.effects.running(after.time_ms).count();
    report.check(
        theirs.force.speed_until <= after.time_ms
            && theirs.force.protect_until <= after.time_ms
            && left == 0,
        format!(
            "Breach's script ends Master Speed, Force Armor and Master Valor at once ({left} effects left)"
        ),
    );
    report.replayed("breach", scene.replayed);

    outln!("  Whirlwind (push's second level, row 27) at {target_tag}");
    let mut whirler = Sheet::dev_jedi();
    whirler.powers = sim::combat::sheet::Rows::default().with(27);
    let whirl_start = stage.scene(target, 160.0, whirler).expect("a clear spot");
    let scene = stage.run(&whirl_start, target, 400, &mut tap(27));
    let after = fighter(scene.snapshots.last().expect("stepped"), target);
    let hurt = after.next_event != fighter(&whirl_start, target).next_event;
    let held = after.force.immune_until > 0;
    report.check(
        held == hurt,
        format!(
            "the Reflex save {}: held {held}, struck {hurt} (a third of level 6: 2 HP)",
            if held { "failed" } else { "made" }
        ),
    );
    report.replayed("whirlwind", scene.replayed);

    // TSL's Fury (JA's rage) and Force Sight, with TSL's costs for their
    // rows (K1's table has none): the module installed again with them.
    outln!("  rage (Master Fury, row 166) and sight (Force Sight, row 176), with TSL's costs");
    let mut tsl = stage.level.module().expect("a module").as_ref().clone();
    tsl.force_costs.resize(177, 0);
    (tsl.force_costs[166], tsl.force_costs[176]) = (30, 15);
    let tsl_level = stage.level.clone().with_module(tsl);
    let tsl_stage = crate::scenes::one_seat(tsl_level, None).map(|mut world| Stage::of(&mut world));
    match tsl_stage.ok().and_then(|tsl| {
        tsl.scene(target, 160.0, Sheet::dev_jedi())
            .map(|start| (tsl, start))
    }) {
        Some((tsl, mut start)) => {
            // The target has the player mind-tricked, to be seen through.
            if let Some(fighter) = start.entities[target].fighter.as_mut() {
                fighter.force.tricked = 1;
                fighter.force.trick_until = start.time_ms + 60_000;
            }
            let scene = tsl.run(&start, target, 2000, &mut |ms| match ms {
                0 => (buttons::FORCE_POWER, 166, 0),
                504 => (buttons::FORCE_POWER, 176, 0),
                1600 => (buttons::FORCE_POWER, 166, 0),
                _ => (0, 0, 0),
            });
            let at = |ms: u64| &scene.snapshots[(ms / 8) as usize];
            let raging = fighter(at(8), me);
            report.check(
                raging.force.raging(at(8).time_ms) == 3
                    && player(at(8), me).run_speed == motion::BASE_SPEED * force::ja::RAGE_SPEED,
                format!(
                    "Master Fury: rage level {}, base speed {}",
                    raging.force.raging(at(8).time_ms),
                    player(at(8), me).run_speed
                ),
            );
            // Level 3 costs 2 JA points of 54 HP every 450 ms: three by 1.4 s
            // (the target's blade may land meanwhile: not the rage's).
            let after = fighter(at(1400), me);
            let my_id = start.entities[me].id;
            let blows: i32 = after
                .events
                .iter()
                .flatten()
                .filter(|e| e.by.is_some_and(|by| by != my_id) && e.tick > start.tick)
                .map(|e| e.damage)
                .sum();
            let lost = fighter(&start, me).hp - after.hp - blows;
            report.check(
                lost == 3 * 2 * 54,
                format!(
                    "rage's cost by 1.4 s: {:.2} HP (3 × 2 JA points of 54)",
                    f64::from(lost) / 100.0
                ),
            );
            let seen = fighter(at(520), target).force.tricked;
            report.check(
                fighter(at(520), me).force.sight_until > at(520).time_ms && seen == 0,
                format!(
                    "Force Sight: running, the target's trick on the player broken (mask {seen})"
                ),
            );
            let after = scene.snapshots.last().expect("stepped");
            let state = fighter(after, me).force;
            report.check(
                state.raging(after.time_ms) == 0
                    && state.recover_until > after.time_ms
                    && player(after, me).run_speed == motion::BASE_SPEED * force::ja::RECOVER_SPEED,
                format!(
                    "turned off at 1.6 s: recovering {} ms more, base speed {}",
                    state.recover_until.saturating_sub(after.time_ms),
                    player(after, me).run_speed
                ),
            );
            report.replayed("rage and sight", scene.replayed);
        }
        None => report.check(
            false,
            "the module with TSL's costs did not install".to_string(),
        ),
    }

    // Force jump (I26, rulebook 4.6.1) in the arena's open air: jump held.
    outln!("  force jump (held 2 s), in the arena");
    let arena = Stage::arena();
    let jump = |sheet: Sheet| {
        let start = arena.with_sheet(sheet);
        let me = seat0(&start);
        let run = arena.drive(&start, 2000, &mut |last, _| {
            let ps = player(last, me);
            Command {
                up: 127,
                ..mind::looking(&ps, 0.0, ps.view[1])
            }
        });
        let base = player(&start, me).origin[2];
        let peak = run
            .snapshots
            .iter()
            .map(|snapshot| player(snapshot, me).origin[2] - base)
            .fold(0.0f32, f32::max);
        let jumped = run
            .snapshots
            .iter()
            .any(|snapshot| fighter(snapshot, me).force.jumping);
        let last = run.snapshots.last().expect("stepped");
        (peak, fighter(last, me).fp, jumped)
    };
    let (jedi_peak, jedi_fp, jedi_active) = jump(Sheet::dev_jedi());
    let (soldier_peak, ..) = jump(Sheet::dev_soldier());
    report.check((192.0..=230.0).contains(&jedi_peak) && jedi_active && jedi_fp < 6000,
        format!(
            "the Guardian (jump level 2) rises {jedi_peak:.0} units (lifted to 192, then carried), the power active, {:.2} FP left of 60",
            f64::from(jedi_fp) / 100.0
        ),
    );
    report.check(
        soldier_peak < 40.0,
        format!("the soldier (no Force) rises {soldier_peak:.0} units: a normal jump"),
    );

    every_power(&stage, target, &mut report);

    report.finish(serde_json::json!({
        "verb": "combat force", "game": game, "module": module,
        "target": target_tag, "drained": drained_tag,
    }))
}
