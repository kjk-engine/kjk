//! The saber animation audit's log (`kjk play --audit-log PATH`): one JSON
//! object per line for each drawn frame of the player's figure, the one
//! `--trace` follows.
//!
//! - **The sequences**: what the sim's skeleton plays on the legs and the
//!   torso, what the drawn body plays in their place (`Figure::shown_parts`,
//!   after `duel_swap`), whether a part was swapped, and whether the saber
//!   move cuts (`Figure::cuts`).
//! - **Contact**: the move cuts, or the snapshot's saber says the character
//!   is blocked (a parry meeting a blow), in a saber lock, or inside a bolt
//!   block. The move's own `blocking` is not read: JA's ready stance blocks
//!   wide too, so it would mark every lit frame.
//! - **The nodes**: the drawn KOTOR body's nodes (the ones of `PAIRS` the
//!   body has) and the JA bones `PAIRS` gives for the same body parts, as the
//!   view's Ghoul2 player poses the body before the retarget, placed at the
//!   drawn origin as `sim_blades` places JA's blade. Game space, metres.
//! - **The blades**: per lit blade in hand of a body posed this frame, the
//!   drawn blade, JA's blade as the view poses it and the sim's blade
//!   (`sim_blades::in_hand`), and the drawn gripping hand. A blade whose
//!   body is held in its last pose is left out, as `sim_blades` leaves it
//!   out of its measures. The drawn blades are known only once they are
//!   drawn (`scene::BladesDrawn`), so the frame begun in the view's `draw`
//!   is finished and written after them (`write`). Game space, metres.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Write as _};
use std::path::Path;
use std::sync::Arc;

use bevy::prelude::*;
use collision::UNIT;
use serde::Serialize;
use sim::skeleton::Hand;
use sim::{SimId, Snapshot};

use crate::cast::{self, Cast};
use crate::scene::DrawnBlades;
use crate::sim_blades::{self, Shown};

/// The drawn KOTOR body's nodes the log follows, each with the bone of JA's
/// skeleton that stands for the same body part. The drawn head hangs on
/// `headhook`. A body with a head of its own has `head_g` as well. Both
/// head nodes stand for `cranium`.
pub const PAIRS: [(&str, &str); 16] = [
    ("pelvis_g", "pelvis"),
    ("head_g", "cranium"),
    ("headhook", "cranium"),
    ("torsoupr_g", "thoracic"),
    ("lhand_g", "lhand"),
    ("rhand_g", "rhand"),
    ("lforearm_g", "lradius"),
    ("rforearm_g", "rradius"),
    ("lbicep_g", "lhumerus"),
    ("rbicep_g", "rhumerus"),
    ("lthigh_g", "lfemurYZ"),
    ("rthigh_g", "rfemurYZ"),
    ("lshin_g", "ltibia"),
    ("rshin_g", "rtibia"),
    ("lfoot_g", "ltalus"),
    ("rfoot_g", "rtalus"),
];

/// The bone of JA's skeleton that stands for the KOTOR `node` (`PAIRS`), if
/// the log follows that node.
pub fn ja_bone(node: &str) -> Option<&'static str> {
    PAIRS
        .iter()
        .find(|(audited, _)| *audited == node)
        .map(|(_, bone)| *bone)
}

/// The bones of `PAIRS`, each once, in the table's order.
fn ja_bones() -> Vec<&'static str> {
    let mut bones: Vec<&'static str> = Vec::with_capacity(PAIRS.len());
    for (_, bone) in PAIRS {
        if !bones.contains(&bone) {
            bones.push(bone);
        }
    }
    bones
}

/// One drawn frame of the player's figure.
#[derive(Debug, Clone, Serialize)]
pub struct AuditFrame {
    /// The drawn moment (sim ms).
    pub ms: f64,
    /// The sim's legs and torso sequence names.
    pub sim: [String; 2],
    /// The legs and torso sequence names the drawn body plays.
    pub shown: [String; 2],
    /// `duel_swap` replaced at least one part.
    pub swapped: bool,
    /// The saber move cuts (`Figure::cuts`).
    pub cuts: bool,
    /// The blade can touch something (`cast::in_contact`). The log reads
    /// the fighter from the last snapshot at or before the figure's own
    /// moment, while the view's turn of the drawn blade
    /// (`Figure::turn_share`) reads it from the first snapshot at or after
    /// it. The two can be one snapshot apart.
    pub contact: bool,
    /// The saber rules in play (`ja` or `mda`).
    pub rules: String,
    /// The drawn body's nodes, by KOTOR name.
    pub nodes: BTreeMap<String, [f32; 3]>,
    /// The same body parts on JA's skeleton, by JA's bone name.
    pub ref_nodes: BTreeMap<String, [f32; 3]>,
    pub blades: Vec<AuditBlade>,
}

/// One lit blade in hand: hilt and tip of each blade, and where the drawn
/// hand that grips it is.
#[derive(Debug, Clone, Serialize)]
pub struct AuditBlade {
    /// Its place among the character's blades (`blades::slots`).
    pub slot: u8,
    /// The drawn blade is still growing or shrinking.
    pub growing: bool,
    pub drawn: [[f32; 3]; 2],
    /// JA's blade as the view poses it.
    pub reference: [[f32; 3]; 2],
    pub sim: [[f32; 3]; 2],
    pub hand: [f32; 3],
}

/// The log's file, this frame's begun frame with its blades still to be
/// drawn, and whether a write failed: the log then stops, so a short log is
/// never taken for a whole one.
#[derive(Resource)]
pub struct AuditLog {
    file: BufWriter<File>,
    pending: Option<(AuditFrame, Vec<Shown>)>,
    failed: bool,
}

impl AuditLog {
    pub fn create(path: &Path) -> io::Result<AuditLog> {
        Ok(AuditLog {
            file: BufWriter::new(File::create(path)?),
            pending: None,
            failed: false,
        })
    }

    pub fn write(&mut self, frame: &AuditFrame) -> io::Result<()> {
        serde_json::to_writer(&mut self.file, frame)?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }

    /// Begins the frame of character `id` at `ms` in `history` (the own
    /// character's from its predictions, `own`): its drawn body's nodes are
    /// `drawn` (each node's name and place, game space, metres), and its
    /// blades among `blades` are finished once drawn (`write`).
    pub fn begin(
        &mut self,
        (cast, history, ms): (&Cast, &[Arc<Snapshot>], f64),
        own: Option<&cast::Own<'_>>,
        id: SimId,
        drawn: &[(&str, Vec3)],
        blades: &[Shown],
    ) {
        self.pending = frame((cast, history, ms), own, id, drawn).map(|frame| {
            let blades = blades.iter().filter(|shown| shown.id == id).copied();
            (frame, blades.collect())
        });
    }
}

/// The frame of `id` at `ms` without its blades (see `AuditLog::begin`).
fn frame(
    (cast, history, ms): (&Cast, &[Arc<Snapshot>], f64),
    own: Option<&cast::Own<'_>>,
    id: SimId,
    drawn: &[(&str, Vec3)],
) -> Option<AuditFrame> {
    let figure = cast.figure(id)?;
    let (snapshots, at_ms) = sim_blades::timeline((history, ms), id, own);
    let (snapshot, _, _) = sim_blades::around(snapshots, at_ms)?;
    let entity = cast::entity(snapshot, id)?;
    let (player, fighter) = (entity.player?, entity.fighter?);
    let sim = [player.legs.anim.name(), player.torso.anim.name()];
    let saber = &fighter.saber;
    let now = at_ms.max(0.0) as u64;
    let contact = cast::in_contact(figure.cuts(), saber, now);
    let origin = cast::blended_for(history, id, ms, own)?.origin;
    let placed = sim_blades::ja_placed(origin);
    let posed = figure.ja_points(at_ms, &ja_bones());
    Some(AuditFrame {
        ms,
        sim: sim.map(String::from),
        shown: figure.shown_parts().unwrap_or(sim).map(String::from),
        swapped: figure.swapped(),
        cuts: figure.cuts(),
        contact,
        rules: snapshot.saber_rules.name().to_string(),
        nodes: by_name(drawn.iter().copied()),
        ref_nodes: by_name(
            posed
                .into_iter()
                .map(|(name, at)| (name, placed.transform_point3(at))),
        ),
        blades: Vec::new(),
    })
}

fn by_name<'n>(points: impl Iterator<Item = (&'n str, Vec3)>) -> BTreeMap<String, [f32; 3]> {
    points
        .map(|(name, at)| (name.to_string(), at.to_array()))
        .collect()
}

/// The KOTOR nodes of `PAIRS` that `names` (a body's, in order) has, each
/// with its index.
pub fn audited_nodes(names: &[&str]) -> Vec<(&'static str, usize)> {
    PAIRS
        .iter()
        .filter_map(|&(node, _)| {
            let index = names.iter().position(|n| n.eq_ignore_ascii_case(node))?;
            Some((node, index))
        })
        .collect()
}

/// Finishes this frame's begun frame with its blades as drawn, and writes
/// it. A blade whose body is held in its last pose is left out, as
/// `sim_blades` leaves it out of its measures. The first write that fails
/// is said and ends the log.
pub fn write(mut log: ResMut<AuditLog>, drawn: Res<DrawnBlades>) {
    let Some((mut frame, shown)) = log.pending.take() else {
        return;
    };
    if log.failed {
        return;
    }
    let metres = |v: Vec3| (v * UNIT).to_array();
    for shown in shown.into_iter().filter(|shown| shown.posed) {
        let Some(ends) = drawn.0.get(shown.spec).copied().flatten() else {
            continue;
        };
        let (Some(ja), Some(hand)) = (
            shown.ja.as_ref(),
            frame.nodes.get(match shown.hand {
                Hand::Right => "rhand_g",
                Hand::Left => "lhand_g",
            }),
        ) else {
            continue;
        };
        let sim = sim_blades::ends(&shown.sim);
        let ja = sim_blades::ends(ja);
        frame.blades.push(AuditBlade {
            slot: shown.slot as u8,
            growing: sim_blades::growing((ends[0] / UNIT, ends[1] / UNIT), sim),
            drawn: ends.map(|end| end.to_array()),
            reference: [metres(ja.0), metres(ja.1)],
            sim: [metres(sim.0), metres(sim.1)],
            hand: *hand,
        });
    }
    if let Err(error) = log.write(&frame) {
        eprintln!("--audit-log: {error}; no frames are written after this one");
        log.failed = true;
    }
}
