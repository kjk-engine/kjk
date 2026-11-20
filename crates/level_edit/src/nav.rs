//! A project's changes to the navigation mesh: the floor the game's
//! characters find their way over (`docs/NAV.md`, `collision::nav`). Play
//! bakes the mesh from the area's walkable faces (`session`); a project's
//! boxes change those faces first, by their role (`area::BoxRole`).
//!
//! - A walkable box's top and a floor box's top join the faces.
//! - The faces under a blocker are cut along its outline, seen from above
//!   (`collision_odyssey::footprints::cut`), and the pieces inside it are
//!   left out.
//!
//! Only the project's own module changes. The player walks on the area's
//! collision whatever the mesh holds.

use crate::area::{BoxRole, CollisionBox};

type Triangle = [[f32; 3]; 3];

/// How far below a blocker's bottom and above its top a face still counts
/// as inside it, metres.
const SLACK: f32 = 0.05;

/// `floor` (the area's walkable faces, `AreaCollision::nav_faces`) changed
/// by `boxes`.
pub fn edited(floor: Vec<Triangle>, boxes: &[CollisionBox]) -> Vec<Triangle> {
    let mut faces = floor;
    for shape in boxes {
        if matches!(shape.role, BoxRole::Walkable | BoxRole::NavFloor) {
            faces.extend(shape.top());
        }
    }
    let blockers: Vec<&CollisionBox> = boxes
        .iter()
        .filter(|shape| shape.role == BoxRole::NavBlock)
        .collect();
    if blockers.is_empty() {
        return faces;
    }
    // Each blocker's outline seen from above, as two triangles that reach
    // from its bottom to its top, so the cut finds the floor anywhere
    // within its height.
    let outlines: Vec<Triangle> = blockers
        .iter()
        .flat_map(|shape| {
            let c = shape.corners();
            [[c[0], c[1], c[7]], [c[0], c[7], c[6]]]
        })
        .collect();
    // A face lying on the blocker's bottom or top counts as inside it.
    let grown: Vec<CollisionBox> = blockers
        .iter()
        .map(|shape| CollisionBox {
            size: [shape.size[0], shape.size[1], shape.size[2] + 2.0 * SLACK],
            ..(*shape).clone()
        })
        .collect();
    collision_odyssey::footprints::cut(&faces, &outlines)
        .into_iter()
        .filter(|face| {
            let middle: [f32; 3] =
                std::array::from_fn(|axis| (face[0][axis] + face[1][axis] + face[2][axis]) / 3.0);
            !grown.iter().any(|shape| shape.holds(middle))
        })
        .collect()
}
