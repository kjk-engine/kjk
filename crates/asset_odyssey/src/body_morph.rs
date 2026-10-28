//! The owner's touch-ups to particular characters' bodies, applied to the
//! rest pose of the copy a creature is drawn with. Skin weights are kept, so
//! the moved vertices still follow their bones. The models themselves are
//! untouched: armour bodies are shared (Bastila's `PFB*M` are every
//! medium female's), and the sim reads them by name.

use std::sync::OnceLock;

use fmt_odyssey::twoda::TwoDa;

use crate::model::{Matrix, MeshIr, ModelIr, invert_rigid};

/// The owner's table (`data/morph/chest.morph`): `appearance.2da` label
/// prefixes and how much each one's chest is enlarged.
const CHEST_FILE: &str = include_str!("../../../data/morph/chest.morph");

/// The chest bone the apex band is placed from (KOTOR's humanoid skeletons'
/// upper torso).
const CHEST_BONE: &str = "torsoupr_g";
/// Where the apex of each side is sought (Z up, +Y forward): up from the
/// chest bone, and out from the body's midline to either side.
const APEX_HEIGHT: (f32, f32) = (-0.020_674, 0.179_326);
const APEX_SIDE: (f32, f32) = (0.02, 0.15);
/// How far behind its apex a side's centre sits, and its reach.
const DEPTH: f32 = 0.07;
const RADIUS: f32 = 0.11;

/// `CHEST_FILE`'s lines: (label prefix, amount), lowercase.
fn table() -> &'static [(String, f32)] {
    static TABLE: OnceLock<Vec<(String, f32)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        CHEST_FILE
            .lines()
            .filter_map(|line| {
                let mut words = line.split('#').next()?.split_whitespace();
                let prefix = words.next()?.to_ascii_lowercase();
                let amount = words.next()?.parse().ok()?;
                Some((prefix, amount))
            })
            .collect()
    })
}

/// How much the chest of `appearance.2da` row `label` is enlarged, if at all.
pub fn chest(label: &str) -> Option<f32> {
    let label = label.to_ascii_lowercase();
    table()
        .iter()
        .find(|(prefix, _)| label.starts_with(prefix.as_str()))
        .map(|&(_, amount)| amount)
}

/// The chest a body model is drawn with when no row is known: the amount
/// every `appearance.2da` row naming it as a body (`modela` … `modelj`)
/// agrees on; none for a body rows disagree on (an armour body every
/// woman of a size shares).
pub fn chest_of_body(appearances: &TwoDa, body: &str) -> Option<f32> {
    let columns: Vec<String> = ('a'..='j').map(|letter| format!("model{letter}")).collect();
    let mut amounts = (0..appearances.cells.len())
        .filter(|&row| {
            columns.iter().any(|column| {
                appearances
                    .resref(row, column)
                    .is_some_and(|model| model.eq_ignore_ascii_case(body))
            })
        })
        .map(|row| {
            appearances
                .text(row, "label")
                .and_then(|label| chest(&label))
        });
    let first = amounts.next()?;
    amounts.all(|amount| amount == first).then_some(first)?
}

/// `ir`'s chest enlarged by `amount` (`chest`).
pub fn apply(ir: &mut ModelIr, amount: f32) {
    let worlds: Vec<Matrix> = ir.meshes.iter().map(|m| ir.nodes[m.node].world).collect();
    let centres = [-1.0, 1.0].map(|side| centre(ir, &worlds, side));
    let [Some(left), Some(right)] = centres else {
        return;
    };
    let torso_joints: Vec<Vec<bool>> = ir.meshes.iter().map(|mesh| torso_slots(ir, mesh)).collect();
    for ((mesh, world), torso) in ir.meshes.iter_mut().zip(&worlds).zip(&torso_joints) {
        let local = invert_rigid(world);
        for (index, position) in mesh.positions.iter_mut().enumerate() {
            let on_torso = match &mesh.skin {
                Some(skin) => match (
                    skin.vertex_joints.get(index),
                    skin.vertex_weights.get(index),
                ) {
                    (Some(joints), Some(weights)) => (0..4)
                        .filter(|&k| torso[usize::from(joints[k])])
                        .map(|k| weights[k])
                        .sum::<f32>(),
                    _ => 0.0,
                },
                None => 1.0,
            };
            if on_torso <= 0.0 {
                continue;
            }
            let mut moved = transform(world, *position);
            for centre in [left, right] {
                let offset = sub(moved, centre);
                let reach = length(offset) / RADIUS;
                if reach >= 1.0 {
                    continue;
                }
                let ease = (1.0 - reach * reach).powi(2);
                let grow = amount * ease * on_torso;
                moved = [0, 1, 2].map(|k| moved[k] + offset[k] * grow);
            }
            *position = transform(&local, moved);
        }
    }
}

/// A side's centre: its most forward torso point in the chest band above
/// the chest bone, set back by `DEPTH`. `side` is -1 for −X, 1 for +X.
fn centre(ir: &ModelIr, worlds: &[Matrix], side: f32) -> Option<[f32; 3]> {
    let bone = ir
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case(CHEST_BONE))?;
    let [_, _, z] = transform(&bone.world, [0.0; 3]);
    let apex = ir
        .meshes
        .iter()
        .zip(worlds)
        .filter(|(mesh, _)| torso_slots(ir, mesh).iter().any(|&torso| torso))
        .flat_map(|(mesh, world)| mesh.positions.iter().map(|&p| transform(world, p)))
        .filter(|p| {
            (APEX_HEIGHT.0..APEX_HEIGHT.1).contains(&(p[2] - z))
                && (APEX_SIDE.0..APEX_SIDE.1).contains(&(p[0] * side))
        })
        .max_by(|a, b| a[1].total_cmp(&b[1]))?;
    Some([apex[0], apex[1] - DEPTH, apex[2]])
}

/// Per skin slot, whether its bone is the chest's (`torso*`, `breastbone`).
fn torso_slots(ir: &ModelIr, mesh: &MeshIr) -> Vec<bool> {
    mesh.skin.as_ref().map_or_else(Vec::new, |skin| {
        skin.joints
            .iter()
            .map(|&node| {
                let name = ir.nodes[node].name.to_ascii_lowercase();
                name.starts_with("torso") || name == "breastbone"
            })
            .collect()
    })
}

fn transform(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
