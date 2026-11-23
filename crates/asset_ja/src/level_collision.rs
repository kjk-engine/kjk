//! A Jedi Academy level's collision (`collision::Level`) and its walkable
//! floor, from the level's file (`fmt_ja::bsp`). Everything is in metres
//! (`collision::UNIT`).
//!
//! **Brushes.** The level keeps each solid as a brush: the planes that
//! bound it. kjk's level builder takes a solid as its corners, its faces
//! and its edges, so each brush is worked back into them. A corner is
//! where three of the brush's planes meet inside all the others. An edge
//! joins two corners that lie on two of the same planes. The corners are
//! worked out in `f64` with `+ - * /` alone, so every machine builds the
//! same level.
//!
//! A brush is level geometry when its shader's contents are solid. It
//! blocks players and creatures only when they are the player clip. A
//! brush of any other contents (water, fog, a trigger) is left out.
//!
//! **Moving parts.** A door's brushes (`movers`) belong to the door, as
//! a KOTOR door's walkmesh does: solid where the door is closed while it
//! is closed (state 0), and solid where it has moved to once it is open
//! (states 1 and 2). The door's number is the entity that owns them. Any
//! other part that moves (a lift, a wall a script switches) is solid
//! where it rests.
//!
//! **Curved surfaces.** The level keeps no brush for a curved surface.
//! The game builds its collision from the surface itself, and so does
//! this: the triangles the surface is drawn with (`level`), each a thin
//! solid, where the surface's shader is solid. The game cuts a curve for
//! collision by a rule of its own, so the two differ by up to the drawing
//! tolerance (`level::PATCH_TOLERANCE`).
//!
//! **The floor.** The triangles the navigation mesh is baked from
//! (`collision::nav`). They are the world's drawn flat and curved surfaces
//! whose shader is solid or the player clip, and which face up at all. The
//! level's compiler has already cut away what lies inside walls, which a
//! brush's own top face would still hold. The mesh keeps the triangles a
//! body can stand on, and steeper ones no taller than a step. A stair's
//! steps meet as edges along one line, which the mesh joins up to a step's
//! height apart (`NavMesh::build_stepped`). Models the compiler put into
//! the level are left out, because they have no collision. The tops of
//! brushes nobody sees (the player clip's, and tops never drawn) were
//! tried as floor too: on `t1_sour`, `kor1`, `vjun2` and `mp/ffa3` they
//! joined no waypoint more, and made the floor up to 17 times larger.
//!
//! A level's terrain is often drawn as triangles of any shape too: Hoth's
//! snow is a shader the compiler turns into such triangles
//! (`q3map_forcemeta`), and its collision is the brushes it was made from.
//! So a triangle of that kind is floor where a solid brush lies right under
//! it (`ON_SOLID`). A model the compiler put into the level stands on the
//! floor, and its top is not over a brush, so it stays out.
//!
//! **The ground.** What a surface looks like underfoot (`collision::Ground`,
//! which footsteps sound by) comes from the material the compiler keeps
//! in each shader's surface flags (`ground_of`). A brush takes the ground
//! of its top side, and a curve or a floor triangle the ground of its own
//! shader.
//!
//! The level's compiler does not cut a floor where a detail brush stands
//! on it, so a stair's lowest edge lies on the floor, which runs on under
//! the stair. A level also draws some places twice, a step's tread and a
//! slope over it. The mesh joins such floors where a body steps across
//! (`NavMesh::bridge`).

use collision::{CollisionWorld, Contents, Extents, Ground, Level, LevelBuilder, Tag, UNIT};
use fmt_ja::bsp::{Bsp, CONTENTS_PLAYER_CLIP, CONTENTS_SOLID, SURFACE_SKY, SurfaceKind};

/// The widest gap between two floors (metres) that the navigation mesh
/// joins across, where a body steps over it.
pub const BRIDGE: f32 = 0.3;

/// How far over and under a triangle of any shape (metres) a solid brush
/// is looked for, for the triangle to count as floor.
const ON_SOLID: f32 = 0.1;

/// The ground that a surface whose shader has the surface flags `flags`
/// looks like underfoot. The compiler keeps the shader's material
/// (`q3map_material`) as a number in the flags' lowest five bits: 14 for
/// Hoth's snow, 23 for its rock, 3 and 4 for solid and hollow metal. A
/// shader with no material, as most are, steps as stone. Glass, concrete,
/// marble, rock, tiles, plaster, rubber and plastic step as stone too.
pub fn ground_of(flags: u32) -> Ground {
    match flags & 0x1f {
        1 | 2 => Ground::Wood,
        3 | 4 | 30 | 31 => Ground::Metal,
        5 | 6 | 19 | 20 => Ground::Grass,
        7 => Ground::Dirt,
        8 => Ground::Sand,
        9 => Ground::Gravel,
        13 => Ground::Water,
        14 | 15 => Ground::Snow,
        16 | 17 => Ground::Mud,
        21 | 22 | 27 => Ground::Rug,
        _ => Ground::Stone,
    }
}

/// How far (units) a corner may lie outside a plane and still be the
/// brush's. The file's planes are single floats, and corners far from the
/// origin are worked out to about a thousandth of a unit.
const ON_PLANE: f64 = 0.05;
/// A triangle whose normal's height is over this faces up. At or under
/// it, the triangle stands upright (a wall) or faces down (a ceiling).
const UPRIGHT: f64 = 0.01;
/// Corners closer than this (units) are one corner.
const SAME_CORNER: f64 = 0.05;

/// A level's collision and floor.
pub struct LevelCollision {
    pub level: Level,
    /// The floor's triangles, wound counter-clockwise seen from above:
    /// every one that faces up at all.
    pub floor: Vec<[[f32; 3]; 3]>,
    /// Each floor triangle's ground (`ground_of`), in the floor's order.
    pub floor_ground: Vec<Ground>,
    pub tally: Tally,
}

/// What a level's collision was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    /// Brushes of level geometry, and brushes that block players and
    /// creatures only.
    pub solid: usize,
    pub player_clip: usize,
    /// Brushes of doors, each added twice: closed and open.
    pub doors: usize,
    /// Brushes left out: of other contents, and those whose planes bound
    /// no solid.
    pub other_contents: usize,
    pub no_solid: usize,
    /// Triangles of curved surfaces added as solids.
    pub curve_triangles: usize,
    /// Triangles of any shape that are floor: a solid brush lies under
    /// them.
    pub loose_floor: usize,
}

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Where three planes meet, if they meet in one point. A plane is its
/// normal and how far it lies from the origin along it.
fn meeting(a: (V, f64), b: (V, f64), c: (V, f64)) -> Option<V> {
    let (bc, ca, ab) = (cross(b.0, c.0), cross(c.0, a.0), cross(a.0, b.0));
    let volume = dot(a.0, bc);
    if volume.abs() < 1e-6 {
        return None;
    }
    Some(std::array::from_fn(|axis| {
        (a.1 * bc[axis] + b.1 * ca[axis] + c.1 * ab[axis]) / volume
    }))
}

/// A brush as the builder takes it: its corners (units), its planes'
/// normals, and its edges as pairs of corners. `None` where the planes
/// bound no solid.
#[allow(clippy::type_complexity)]
fn hull(planes: &[(V, f64)]) -> Option<(Vec<V>, Vec<[f32; 3]>, Vec<(usize, usize)>)> {
    let mut corners: Vec<V> = Vec::new();
    for i in 0..planes.len() {
        for j in i + 1..planes.len() {
            for k in j + 1..planes.len() {
                let Some(point) = meeting(planes[i], planes[j], planes[k]) else {
                    continue;
                };
                let inside =
                    (planes.iter()).all(|(normal, dist)| dot(*normal, point) - dist <= ON_PLANE);
                let known = corners.iter().any(|corner| {
                    (0..3).all(|axis| (corner[axis] - point[axis]).abs() <= SAME_CORNER)
                });
                if inside && !known {
                    corners.push(point);
                }
            }
        }
    }
    if corners.len() < 4 {
        return None;
    }
    // The planes each corner lies on.
    let on: Vec<Vec<usize>> = corners
        .iter()
        .map(|corner| {
            (0..planes.len())
                .filter(|&plane| {
                    (dot(planes[plane].0, *corner) - planes[plane].1).abs() <= ON_PLANE
                })
                .collect()
        })
        .collect();
    let mut edges = Vec::new();
    for a in 0..corners.len() {
        for b in a + 1..corners.len() {
            let shared = on[a].iter().filter(|plane| on[b].contains(plane)).count();
            if shared >= 2 {
                edges.push((a, b));
            }
        }
    }
    let faces = (planes.iter())
        .map(|(normal, _)| normal.map(|part| part as f32))
        .collect();
    Some((corners, faces, edges))
}

/// What kjk's collision makes of a shader's contents, if anything.
/// Whether a shader with the content flags `flags` stops a body: it is
/// solid or a player clip. Water, fog and the like are walked through.
pub fn stops(flags: u32) -> bool {
    contents(flags).is_some()
}

fn contents(flags: u32) -> Option<Contents> {
    if flags & CONTENTS_SOLID != 0 {
        Some(Contents::SOLID)
    } else if flags & CONTENTS_PLAYER_CLIP != 0 {
        Some(Contents::PLAYER_CLIP)
    } else {
        None
    }
}

impl LevelCollision {
    /// The floor's navigation mesh for bodies `radius` metres wide either
    /// side, with what stands on it marked. Floors that a narrow gap parts,
    /// [`BRIDGE`] metres across or less, are joined where a body steps over
    /// the gap (`NavMesh::bridge`).
    pub fn nav(&self, radius: f32) -> collision::nav::NavMesh {
        let mut mesh = joined_nav(&self.floor, &self.level, radius);
        mesh.mark(&self.level);
        mesh
    }
}

/// The navigation mesh of the floor `floor` for bodies `radius` metres
/// wide either side, with Jedi Academy's floors joined as a body walks
/// them in `level`. Floors whose edges meet within a step's height are
/// joined (`NavMesh::build_stepped`), and so are floors that a narrow gap
/// of [`BRIDGE`] metres or less parts (`NavMesh::bridge`). A KOTOR area
/// that holds rooms cut from a Jedi Academy level is joined so too. What
/// stands on the floor is not marked yet.
pub fn joined_nav(
    floor: &[[[f32; 3]; 3]],
    level: &collision::Level,
    radius: f32,
) -> collision::nav::NavMesh {
    let step = collision::player::STEP_HEIGHT * UNIT;
    let mut mesh = collision::nav::NavMesh::build_stepped(floor, radius, step);
    mesh.bridge(level, BRIDGE, step);
    mesh
}

/// `bsp`'s collision and floor.
pub fn collision(bsp: &Bsp) -> LevelCollision {
    let mut tally = Tally::default();
    let mut builder = LevelBuilder::new();
    let rests = crate::level::rests(bsp);
    let movers = crate::movers::movers(bsp);
    let parts = crate::movers::door_parts(&movers);
    let doors = crate::movers::doors(&movers);
    let metres = |point: V, offset: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|axis| {
            ((point[axis] + f64::from(offset[axis])) * f64::from(UNIT)) as f32
        })
    };

    for (index, model) in bsp.models.iter().enumerate() {
        let offset = rests.get(&index).copied().unwrap_or([0.0; 3]);
        // A door's part: its door's number, and how far it moves.
        let door = parts
            .get(&index)
            .map(|&(door, part)| (door as u32, doors[door].parts[part].1));
        let (first, count) = model.brushes;
        for brush in &bsp.brushes[first as usize..(first + count) as usize] {
            let shader = bsp.shaders.get(brush.shader as usize);
            let Some(contents) = shader.and_then(|shader| contents(shader.content_flags)) else {
                tally.other_contents += 1;
                continue;
            };
            let (first, count) = brush.sides;
            let mut planes: Vec<(V, f64)> = Vec::new();
            for side in &bsp.brush_sides[first as usize..(first + count) as usize] {
                let plane = bsp.planes[side.plane as usize];
                let plane = (plane.normal.map(f64::from), f64::from(plane.dist));
                if !planes.contains(&plane) {
                    planes.push(plane);
                }
            }
            let Some((corners, faces, edges)) = hull(&planes) else {
                tally.no_solid += 1;
                continue;
            };
            let at = |moved: [f32; 3]| -> Vec<[f32; 3]> {
                (corners.iter())
                    .map(|corner| {
                        metres(
                            *corner,
                            std::array::from_fn(|axis| offset[axis] + moved[axis]),
                        )
                    })
                    .collect()
            };
            // The brush's ground: its top side's, the side that faces up
            // the most.
            let ground = (bsp.brush_sides[first as usize..(first + count) as usize].iter())
                .max_by(|a, b| {
                    let up =
                        |side: &&fmt_ja::bsp::BrushSide| bsp.planes[side.plane as usize].normal[2];
                    up(a).total_cmp(&up(b))
                })
                .and_then(|side| bsp.shaders.get(side.shader as usize))
                .map_or(Ground::Stone, |shader| ground_of(shader.surface_flags));
            let added = match door {
                Some((door, moves)) => {
                    let owned = |states: u8| Tag {
                        entity: Some(door),
                        states,
                        ..Tag::level(contents, 0).on(ground)
                    };
                    let closed = builder.convex(&at([0.0; 3]), &faces, &edges, owned(1));
                    let open = builder.convex(&at(moves), &faces, &edges, owned(0b110));
                    tally.doors += usize::from(closed && open);
                    closed && open
                }
                None => {
                    let added = builder.convex(
                        &at([0.0; 3]),
                        &faces,
                        &edges,
                        Tag::level(contents, 0).on(ground),
                    );
                    match (added, contents) {
                        (true, Contents::SOLID) => tally.solid += 1,
                        (true, _) => tally.player_clip += 1,
                        (false, _) => {}
                    }
                    added
                }
            };
            if !added {
                tally.no_solid += 1;
            }
        }
    }

    let mut floor = Vec::new();
    let mut floor_ground = Vec::new();
    // Triangles of any shape that face up, with their ground: floor where
    // a solid brush lies under them, once the brushes are built.
    let mut loose: Vec<([[f32; 3]; 3], Ground)> = Vec::new();
    for surface in bsp.model_surfaces(0) {
        let shader = &bsp.shaders[surface.shader as usize];
        let Some(contents) = contents(shader.content_flags) else {
            continue;
        };
        if !matches!(
            surface.kind,
            SurfaceKind::Planar | SurfaceKind::Patch | SurfaceKind::Triangles
        ) || shader.surface_flags & SURFACE_SKY != 0
        {
            continue;
        }
        let ground = ground_of(shader.surface_flags);
        let (vertices, triangles) = crate::level::triangles(bsp, surface);
        for triangle in triangles {
            let corners = triangle
                .map(|index| metres(vertices[index as usize].position.map(f64::from), [0.0; 3]));
            if surface.kind == SurfaceKind::Patch
                && contents == Contents::SOLID
                && builder.triangle(corners, Tag::level(contents, 0).on(ground))
            {
                tally.curve_triangles += 1;
            }
            let [a, b, c] = corners.map(|corner| corner.map(f64::from));
            let normal = cross(
                std::array::from_fn(|axis| b[axis] - a[axis]),
                std::array::from_fn(|axis| c[axis] - a[axis]),
            );
            let length = dot(normal, normal).sqrt();
            if length > 0.0 && normal[2] / length > UPRIGHT {
                if surface.kind == SurfaceKind::Triangles {
                    loose.push((corners, ground));
                } else {
                    floor.push(corners);
                    floor_ground.push(ground);
                }
            }
        }
    }
    let level = builder.build();
    for (corners, ground) in loose {
        let middle: [f32; 3] = std::array::from_fn(|axis| {
            (corners[0][axis] + corners[1][axis] + corners[2][axis]) / 3.0
        });
        let over = [middle[0], middle[1], middle[2] + ON_SOLID];
        let under = [middle[0], middle[1], middle[2] - ON_SOLID];
        let on_solid = [Contents::SOLID, Contents::PLAYER_CLIP]
            .into_iter()
            .any(|mask| level.trace(over, under, &Extents::POINT, mask).fraction < 1.0);
        if on_solid {
            tally.loose_floor += 1;
            floor.push(corners);
            floor_ground.push(ground);
        }
    }

    LevelCollision {
        level,
        floor,
        floor_ground,
        tally,
    }
}
