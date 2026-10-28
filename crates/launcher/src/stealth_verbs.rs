//! `kjk combat stealth <game> [module]`: stealth and Awareness checked end
//! to end against a module's own creatures (`world::covert`,
//! `world::scripts::perceive`; the Sith base, `tar_m09aa`, by default),
//! with its own scripts. The player is the dev soldier given Stealth 8
//! and a stealth unit (`UNIT`), worn by its own equip action.
//!
//! 1. Plain, 10 m from a Sith (the first spot round it where it sees
//!    the player within a second): seen.
//! 2. On the same spot in stealth mode (X), the rolls at their middle:
//!    unseen and unheard for 5 s.
//! 3. The Sith looks at KOTOR's cadence: every 300 to 699 ms, 14 to 34
//!    looks in 10 s.
//! 4. The opener: stealthed and bare-handed (sneak attack's first tier) a
//!    metre from the Sith, turned hostile and unaware of the player, the
//!    first blow is a sneak attack ("Sneak Attack!") and the opener spent.

use std::process::ExitCode;

use serde_json::json;
use sim::combat::sheet::{factions, feats, skills};
use sim::locks::lines;
use sim::module::ObjectKind;
use sim::notice::Notice;
use vfs::Discovery;

use crate::probe::Probe;
use crate::scenes::Report;
use crate::script_verbs::open;

/// The stealth unit worn (K1's Eriadu Prototype: a belt).
const UNIT: &str = "g_i_belt008";
/// The belt's slot.
const BELT: u8 = 10;
/// Ticks a second.
const SECOND: u64 = sim::TICK_HZ as u64;

/// The player at (`x`, `y`) on floor height `z` (metres), still, its view
/// as its command gives it.
fn stand(probe: &mut Probe, [x, y, z]: [f32; 3]) -> Result<(), String> {
    probe.edit_me(|me| {
        let player = me.player.as_mut().ok_or("no player")?;
        player.origin = [
            x / collision::UNIT,
            y / collision::UNIT,
            z / collision::UNIT + 0.125 - sim::motion::SOLE_Z,
        ];
        player.velocity = [0.0; 3];
        player.view_offset = [0; 3];
        Ok(())
    })
}

/// Whether `watcher` saw the player in the scripts' state.
fn sees(probe: &Probe, watcher: sim::SimId) -> (bool, bool) {
    probe
        .world
        .script_state()
        .perceived
        .get(&watcher.0)
        .map_or((false, false), |perceived| {
            (
                perceived.seen.contains(&probe.me.0),
                perceived.heard.contains(&probe.me.0),
            )
        })
}

/// Steps up to `ticks`, holding the player at `at`: the step the watcher
/// first saw it, if it did.
fn watched(
    probe: &mut Probe,
    watcher: sim::SimId,
    at: [f32; 3],
    ticks: u64,
) -> Result<Option<u64>, String> {
    for tick in 1..=ticks {
        stand(probe, at)?;
        probe.step(1);
        if sees(probe, watcher).0 {
            return Ok(Some(tick));
        }
    }
    Ok(None)
}

/// The watcher's and the player's perception rolls set to their middle,
/// not drawn again for 20 s.
fn mean_rolls(probe: &mut Probe, watcher: sim::SimId) {
    let mut state = probe.world.script_state().clone();
    let now = probe.world.snapshot().time_ms;
    for id in [watcher.0, probe.me.0] {
        state
            .perception
            .insert(id, (now, sim::stealth::Rolls::MEAN));
    }
    probe.world.set_script_state(state);
}

/// The creature turned to `faction`, and made to forget whom it fought.
fn turn(probe: &mut Probe, id: sim::SimId, faction: u8) -> Result<(), String> {
    probe.edit(id, |entity| {
        let fighter = entity.fighter.as_mut().ok_or("no fighter")?;
        fighter.faction = Some(faction);
        fighter.target = None;
        fighter.combat_until = 0;
        Ok(())
    })
}

/// Stealth mode turned on (X): whether it took.
fn hide(probe: &mut Probe) -> bool {
    let (snapshot, _) = probe.press(sim::Command {
        buttons: sim::buttons::STEALTH,
        ..sim::Command::default()
    });
    probe.step(2);
    snapshot
        .get(probe.me)
        .and_then(|me| me.fighter.as_ref())
        .is_some_and(|fighter| fighter.covert.stealth)
}

pub fn stealth(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, ..) = open(discovery, game)?;
        let mut probe = Probe::own(discovery, game, module)?;
        let level = probe.world.level().clone();
        let library = level.scripts().cloned().ok_or("the module has no scripts")?;
        let start = probe.step(2);
        // A Sith with a fighter, calmed (the player's faction) so the
        // checks are of sight, not of a fight.
        let creatures: Vec<(sim::SimId, [f32; 3], String)> = start
            .entities
            .iter()
            .filter(|entity| entity.fighter.is_some())
            .filter_map(|entity| match entity.kind {
                sim::Kind::Object(ObjectKind::Creature, index) => {
                    let tag = level.placed().get(index as usize)?.tag.clone();
                    Some((entity.id, entity.position, tag))
                }
                _ => None,
            })
            .collect();
        for (id, ..) in &creatures {
            turn(&mut probe, *id, factions::PLAYER)?;
        }
        probe.edit_me(|me| {
            me.sheet.as_mut().ok_or("no sheet")?.skills[skills::STEALTH] = 8;
            Ok(())
        })?;

        // 1. Plain, 10 m off: the first Sith, and spot round it, it sees
        // the player from.
        let mut found = None;
        let siths = creatures
            .iter()
            .filter(|(.., tag)| tag.to_ascii_lowercase().contains("sith"));
        'search: for (sith, post, tag) in siths {
            for eighth in 0..8 {
                let turn = (eighth as f32 * 45.0).to_radians();
                let spot = [
                    post[0] + 10.0 * turn.cos(),
                    post[1] + 10.0 * turn.sin(),
                    post[2],
                ];
                if let Some(after) = watched(&mut probe, *sith, spot, SECOND)? {
                    found = Some((*sith, *post, tag.clone(), spot, after));
                    break 'search;
                }
            }
        }
        let (sith, post, tag, spot, after) =
            found.ok_or("no Sith that sees the player from 10 m")?;
        report.check(
            after <= SECOND,
            format!(
                "plain, 10 m from the Sith ({tag}) at {spot:?}: seen after {:.2} s",
                after as f32 / SECOND as f32
            ),
        );

        // 2. Stealthed on the same spot.
        let mut state = probe.world.script_state().clone();
        let unit = state
            .make_item(library.items(), UNIT, sim::inventory::PARTY, 1, None)
            .ok_or(format!("no {UNIT}"))?;
        // Forgotten: the Sith's perception starts over.
        state.perceived.remove(&sith.0);
        probe.world.set_script_state(state);
        probe.act(sim::action::Action::Equip {
            item: unit.0,
            slot: BELT,
        });
        probe.step(2);
        stand(&mut probe, spot)?;
        let hidden = hide(&mut probe);
        mean_rolls(&mut probe, sith);
        let seen = watched(&mut probe, sith, spot, 5 * SECOND)?;
        let heard = sees(&probe, sith).1;
        report.check(
            hidden && seen.is_none() && !heard,
            format!(
                "with Stealth 8 and {UNIT} worn, X: in stealth mode {hidden}; the rolls their middle, on the same spot for 5 s: seen {seen:?}, heard {heard}"
            ),
        );

        // 3. The cadence.
        let mut looks = 0;
        let mut last = None;
        for _ in 0..10 * SECOND {
            stand(&mut probe, spot)?;
            probe.step(1);
            let now = probe
                .world
                .script_state()
                .perception
                .get(&sith.0)
                .map(|(at, _)| *at);
            if now != last {
                looks += 1;
                last = now;
            }
        }
        report.check(
            (14..=34).contains(&looks),
            format!("the Sith looked {looks} times in 10 s (every 300 to 699 ms: 14 to 34)"),
        );

        // 4. The opener.
        probe.edit_me(|me| {
            let sheet = me.sheet.as_mut().ok_or("no sheet")?;
            sheet.weapons = [None; 2];
            sheet.feats = sheet.feats.with(feats::SNEAK_ATTACK[0]);
            Ok(())
        })?;
        turn(&mut probe, sith, factions::HOSTILE_1)?;
        let close = [post[0] + 1.0, post[1], post[2]];
        let yaw = (post[1] - close[1]).atan2(post[0] - close[0]).to_degrees();
        let facing = sim::Command {
            angles: sim::moves::view_angles(0.0, yaw),
            ..sim::Command::default()
        };
        let mut sneaks = 0;
        let mut opener = None;
        for tick in 0..6 * SECOND {
            stand(&mut probe, close)?;
            mean_rolls(&mut probe, sith);
            let buttons = if tick >= 4 { sim::buttons::ATTACK } else { 0 };
            let (snapshot, told) = probe.press(sim::Command { buttons, ..facing });
            sneaks += told
                .iter()
                .filter(|notice| **notice == Notice::Line(lines::SNEAK_ATTACK))
                .count();
            opener = snapshot
                .get(probe.me)
                .and_then(|me| me.fighter.as_ref())
                .map(|fighter| fighter.covert.opener_until);
        }
        report.check(
            sneaks >= 1 && opener == Some(0),
            format!("stealthed, bare-handed, a metre from the unaware Sith, attacking: told \"Sneak Attack!\" {sneaks} time(s), the opener spent: {opener:?}"),
        );
        Ok(json!({ "module": module, "sith": sith.0, "spot": spot, "looks": looks }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
