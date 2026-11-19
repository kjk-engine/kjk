//! The probes of a playthrough checkpoint written as a player's save (`kjk
//! sim probes`, group "checkpoint saves"). The K1 chain wrote its
//! checkpoints as saves `k1-01` to `k1-19` with the player still immortal
//! and dealing ten times the damage, so a playtest from them showed no real
//! balance.

use std::path::Path;

use sim::combat::sheet::flags::{IMMORTAL, MIGHTY};
use sim::probe::{Checks, check};

use crate::{read, write_checkpoint};

/// In a folder of its own under the system's temporary one, a checkpoint
/// made on the proving ground with the player immortal and mighty (as
/// `--god --mighty` make it) is written as a save (`write_checkpoint`):
/// - the save reads back whole (its hash matches its world), named, titled
///   and placed as the checkpoint, and its player has neither flag, while
///   everything else in its snapshot is as the checkpoint had it;
/// - the world loaded from the save has a player with neither flag;
/// - the checkpoint's own bytes are unchanged and still carry both flags,
///   so the chain's next route resumes from them as before.
pub fn probes() -> Checks {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("kjk-checkpoint-{}-{run}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let checks = match written(&root) {
        Ok(checks) => checks,
        Err(why) => vec![("checkpoint_world", Err(why))],
    };
    let _ = std::fs::remove_dir_all(&root);
    checks
}

/// The aids on the players' sheets in `snapshot`, one entry a player.
fn aids(snapshot: &sim::Snapshot) -> Vec<u8> {
    (snapshot.entities.iter())
        .filter(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
        .map(|entity| {
            entity
                .sheet
                .map_or(0, |sheet| sheet.flags & (IMMORTAL | MIGHTY))
        })
        .collect()
}

fn written(root: &Path) -> Result<Checks, String> {
    let level = sim::determinism::proving_ground();
    let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), level.clone())
        .map_err(|error| error.to_string())?;
    world.add_player_flags(IMMORTAL | MIGHTY);
    let checkpoint = sim::Save {
        hz: sim::TICK_HZ,
        level: world.level().id(),
        script: world.script_state().clone(),
        snapshot: world.snapshot(),
        stances: world.stance_ids(),
    };
    let bytes = checkpoint.encode();
    write_checkpoint(
        root,
        "k1-01",
        &bytes,
        ("K1-01 Probe", "The proving ground"),
        7,
    )?;
    let (save, meta) = read(root, "k1-01")?;
    let mut expected = checkpoint.snapshot.clone();
    expected.without_test_aids();
    let loaded = save.world(&level, 1, None)?.snapshot();
    let kept = sim::Save::decode(&bytes).map_err(|error| error.to_string())?;
    Ok(vec![
        (
            "save_without_aids",
            check(
                aids(&checkpoint.snapshot) == [IMMORTAL | MIGHTY]
                    && aids(&save.snapshot) == [0]
                    && save.snapshot == expected
                    && meta.name == "K1-01 Probe"
                    && meta.area == "The proving ground"
                    && meta.module == level.id().name
                    && meta.saved == 7
                    && meta.seats == 1,
                format!(
                    "aids {:?} → {:?}, meta {} / {} / {}",
                    aids(&checkpoint.snapshot),
                    aids(&save.snapshot),
                    meta.name,
                    meta.area,
                    meta.module
                ),
            ),
        ),
        (
            "loaded_world_without_aids",
            check(aids(&loaded) == [0], format!("aids {:?}", aids(&loaded))),
        ),
        (
            "checkpoint_keeps_aids",
            check(
                aids(&kept.snapshot) == [IMMORTAL | MIGHTY] && kept == checkpoint,
                format!("aids {:?}", aids(&kept.snapshot)),
            ),
        ),
    ])
}
