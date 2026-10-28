//! Where a model carries things: a tag surface's frame and its bone.

use std::collections::BTreeMap;

use asset_odyssey::model::{IDENTITY, Matrix};
use collision::vector::{cross, dot, normalize, sub};
use fmt_ja::ghoul2::Glm;

use crate::UNIT;

/// A bolt on a model: the bone that carries it, its frame at the base pose
/// (model space, metres: the columns along, across and out of the tag, then
/// its place), and whether the tag moves with that bone alone.
#[derive(Debug, Clone)]
pub struct Bolt {
    pub bone: usize,
    pub matrix: Matrix,
    pub rigid: bool,
}

/// The bolt of tag surface `name` (any case), from its first three
/// vertices in the first LOD: along the side from the third back to the
/// first, across toward the second, out of the triangle's face, placed at
/// the third. Its bone is the one weighing most on those vertices (the
/// highest of a tie). `None` without the surface, three vertices, or a
/// valid bone.
pub fn bolt(glm: &Glm, name: &str) -> Option<Bolt> {
    let index = glm
        .surfaces
        .iter()
        .position(|surface| surface.name.eq_ignore_ascii_case(name))?;
    let surface = glm.lods.first()?.get(index)?;
    if surface.positions.len() < 3 {
        return None;
    }
    let p: [[f32; 3]; 3] = std::array::from_fn(|i| surface.positions[i].map(|v| v * UNIT));
    let unit = |v: [f32; 3]| normalize(v).unwrap_or(v);
    let sides: [[f32; 3]; 3] = std::array::from_fn(|j| sub(p[(j + 1) % 3], p[j]));
    let along = unit(sides[2]);
    let first = unit(sides[0]);
    let d = dot(first, along);
    let across = unit(sub(first, along.map(|c| c * d)));
    let normal = unit(cross(sides[0], sides[2]));
    let mut matrix = IDENTITY;
    for (r, row) in matrix.iter_mut().take(3).enumerate() {
        row[0] = along[r];
        row[1] = across[r];
        row[2] = -normal[r];
        row[3] = p[2][r];
    }

    let mut totals: BTreeMap<i32, f32> = BTreeMap::new();
    let mut rigid = true;
    let mut previous = None;
    for &(reference, weight) in surface.weights.iter().take(3).flatten() {
        let bone = *surface.bone_references.get(usize::from(reference))?;
        *totals.entry(bone).or_insert(0.0) += weight;
        if weight < 0.999 || previous.is_some_and(|before| before != bone) {
            rigid = false;
        }
        previous = Some(bone);
    }
    let (&bone, _) = totals
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1).then(a.0.cmp(b.0)))?;
    Some(Bolt {
        bone: usize::try_from(bone).ok()?,
        matrix,
        rigid,
    })
}
