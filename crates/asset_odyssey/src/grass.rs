//! The grass of a room, placed as the games' executables place it: the faces
//! of the room's walkmesh whose surface is grassy, and on each a number of
//! clusters drawn from a generator seeded by the face itself. Nothing here
//! draws; coordinates stay in the games' space (Z up, metres).

use crate::module::{AreaGrass, check};
use crate::msvc_rand::MsvcRand;
use fmt_odyssey::mdl::{Model, Node};

/// A grass face of a room, in the room model's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrassFace {
    pub corners: [[f32; 3]; 3],
    pub lightmap: [[f32; 2]; 3],
}

/// A room's grass faces and the lightmap they are lit by (the AABB mesh's).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrassFaces {
    pub faces: Vec<GrassFace>,
    pub lightmap: Option<String>,
}

/// One cluster: where it stands (world space), its turn about the vertical
/// (radians), its quarter of the texture (0 lower left, 1 lower right,
/// 2 upper left, 3 upper right: the ARE's LL, LR, UL, UR), its size factor
/// (0.8 to 1.2) and the lightmap UV under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuft {
    pub position: [f32; 3],
    pub yaw: f32,
    pub variant: u8,
    pub size: f32,
    pub lightmap_uv: [f32; 2],
}

/// A node's place in the room model: a turn (w, x, y, z), then a shift.
#[derive(Clone, Copy)]
struct Placement {
    turn: [f32; 4],
    shift: [f32; 3],
}

impl Placement {
    const IDENTITY: Self = Self {
        turn: [1.0, 0.0, 0.0, 0.0],
        shift: [0.0; 3],
    };

    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let [w, x, y, z] = self.turn;
        let q = [x, y, z];
        let t = cross(q, p).map(|c| c * 2.0);
        let c = cross(q, t);
        std::array::from_fn(|i| p[i] + w * t[i] + c[i] + self.shift[i])
    }

    /// The placement of a child of this node.
    fn child(&self, node: &Node) -> Self {
        let [aw, ax, ay, az] = self.turn;
        let [bw, bx, by, bz] = node.orientation;
        Self {
            turn: [
                aw * bw - ax * bx - ay * by - az * bz,
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
            ],
            shift: self.point(node.position),
        }
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The first node with an AABB mesh, depth first, and its placement.
fn walkmesh_node<'a>(node: &'a Node, parent: &Placement) -> Option<(&'a Node, Placement)> {
    let here = parent.child(node);
    if node.mesh.as_ref().is_some_and(|mesh| mesh.aabb.is_some()) {
        return Some((node, here));
    }
    node.children
        .iter()
        .find_map(|child| walkmesh_node(child, &here))
}

/// The faces of the room's walkmesh (its AABB mesh) whose surface material
/// grows grass, in the model's space, and the lightmap the mesh is lit by.
pub fn faces(model: &Model, grass: &[bool]) -> GrassFaces {
    let Some((node, placement)) = walkmesh_node(&model.root, &Placement::IDENTITY) else {
        return GrassFaces::default();
    };
    let Some(mesh) = node.mesh.as_ref() else {
        return GrassFaces::default();
    };
    let lightmap =
        Some(mesh.textures[1].to_lowercase()).filter(|name| !name.is_empty() && name != "null");
    let faces = mesh
        .faces
        .iter()
        .filter(|face| grass.get(face.material as usize) == Some(&true))
        .filter_map(|face| {
            let corner = |i: usize| mesh.positions.get(usize::from(face.vertices[i])).copied();
            let uv = |i: usize| {
                mesh.uv2
                    .get(usize::from(face.vertices[i]))
                    .copied()
                    .unwrap_or([0.0; 2])
            };
            Some(GrassFace {
                corners: [
                    placement.point(corner(0)?),
                    placement.point(corner(1)?),
                    placement.point(corner(2)?),
                ],
                lightmap: [uv(0), uv(1), uv(2)],
            })
        })
        .collect();
    GrassFaces { faces, lightmap }
}

/// The tufts of a room placed at `offset`, as the executable places them.
pub fn tufts(faces: &GrassFaces, grass: &AreaGrass, offset: [f32; 3]) -> Vec<Tuft> {
    // The executable's order of the variants: LL, LR, UL, UR; the ARE's
    // chances are UL, UR, LL, LR.
    let [ul, ur, ll, lr] = grass.chances;
    let chances = [ll, lr, ul, ur];
    let total: f32 = chances.iter().sum();
    let mut out = Vec::new();
    for face in &faces.faces {
        let [v0, v1, v2] = face
            .corners
            .map(|c| std::array::from_fn(|i| c[i] + offset[i]));
        let edge = |to: [f32; 3]| std::array::from_fn(|i| to[i] - v0[i]);
        let normal = cross(edge(v1), edge(v2));
        let area = 0.5 * normal.iter().map(|c| c * c).sum::<f32>().sqrt();
        let count = (area * grass.density) as usize;
        let mut rand = MsvcRand((v0[0] + v1[1] + v2[2]) as i32 as u32);
        for _ in 0..count {
            let draws = [rand.unit(), rand.unit(), rand.unit()];
            let sum: f32 = draws.iter().sum();
            let weights = if sum > 0.0 {
                draws.map(|d| d / sum)
            } else {
                [1.0 / 3.0; 3]
            };
            let blend = |a: &[f32], b: &[f32], c: &[f32], i: usize| {
                weights[0] * a[i] + weights[1] * b[i] + weights[2] * c[i]
            };
            let yaw = rand.unit() * std::f32::consts::TAU;
            let r = rand.unit();
            let mut running = 0.0;
            let variant = chances
                .iter()
                .position(|chance| {
                    running += chance;
                    total > 0.0 && running / total > r
                })
                .unwrap_or(3) as u8;
            let size = 1.0 + (rand.unit() - 0.5) * 0.4;
            out.push(Tuft {
                position: std::array::from_fn(|i| blend(&v0, &v1, &v2, i)),
                yaw,
                variant,
                size,
                lightmap_uv: std::array::from_fn(|i| {
                    blend(&face.lightmap[0], &face.lightmap[1], &face.lightmap[2], i)
                }),
            });
        }
    }
    out
}

fn aabb_model(materials: [u32; 2], with_aabb: bool) -> Model {
    use fmt_odyssey::mdl::{AabbNode, Face, Mesh};
    let positions = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [10.0, 10.0, 0.0],
    ];
    let face = |material, vertices| Face {
        normal: [0.0, 0.0, 1.0],
        distance: 0.0,
        material,
        adjacent: [0xffff; 3],
        vertices,
    };
    let node = |name: &str, position, mesh| Node {
        flags: 0,
        number: 0,
        name: name.into(),
        position,
        orientation: [1.0, 0.0, 0.0, 0.0],
        controllers: Vec::new(),
        light: None,
        reference: None,
        emitter: None,
        mesh,
        children: Vec::new(),
        animated: false,
    };
    let mesh = Mesh {
        bounds: [0.0; 6],
        textures: ["".into(), "lm_room".into(), "".into(), "".into()],
        diffuse: [1.0; 3],
        ambient: [1.0; 3],
        transparency_hint: 0,
        render: true,
        shadow: false,
        lightmapped: true,
        background: false,
        beaming: false,
        rotate_texture: false,
        animate_uv: None,
        uv2: positions
            .iter()
            .map(|p| [p[0] / 10.0, p[1] / 10.0])
            .collect(),
        positions,
        normals: Vec::new(),
        uv1: Vec::new(),
        faces: vec![face(materials[0], [0, 1, 2]), face(materials[1], [1, 3, 2])],
        indices: Vec::new(),
        skin: None,
        dangly: None,
        aabb: with_aabb.then_some(AabbNode {
            bounds: [0.0; 6],
            face: -1,
            plane: 0,
            children: None,
        }),
        saber: false,
    };
    let mut root = node("root", [0.0; 3], None);
    root.children
        .push(node("walkmesh", [1.0, 2.0, 0.0], Some(mesh)));
    Model {
        name: "room".into(),
        classification: 0,
        subclassification: 0,
        affected_by_fog: false,
        supermodel: None,
        bounding_box: [0.0; 6],
        radius: 0.0,
        animation_scale: 1.0,
        root,
        animations: Vec::new(),
    }
}

/// The checks of a room's grass faces and the tufts on them, from rooms made
/// here.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let grass_rows = [false, false, false, true];
    let area = |density, chances| AreaGrass {
        texture: "g".into(),
        density,
        quad_size: 1.0,
        ambient: 0,
        diffuse: 0,
        emissive: 0,
        chances,
    };
    let mixed = faces(&aabb_model([3, 1], true), &grass_rows);
    let both = faces(&aabb_model([3, 3], true), &grass_rows);
    let first = tufts(&both, &area(4.0, [0.25; 4]), [0.0; 3]);
    let again = tufts(&both, &area(4.0, [0.25; 4]), [0.0; 3]);
    let moved = tufts(&both, &area(4.0, [0.25; 4]), [7.0, 0.0, 0.0]);
    let thin = tufts(&both, &area(0.019, [0.25; 4]), [0.0; 3]); // 50 × 0.019 = 0.95 → none
    // UL, UR, LL, LR = 0, 0, 1, 0: every tuft lower left (index 0 in the executable's order).
    let lower_left = tufts(&both, &area(4.0, [0.0, 0.0, 1.0, 0.0]), [0.0; 3]);
    let mut rand = MsvcRand(1);
    let sequence = [rand.next(), rand.next(), rand.next()];
    vec![
        (
            "grass_faces_by_material",
            check(
                mixed.faces.len() == 1 && mixed.lightmap.as_deref() == Some("lm_room"),
                format!("{} faces, {:?}", mixed.faces.len(), mixed.lightmap),
            ),
        ),
        (
            "grass_no_material",
            check(
                faces(&aabb_model([1, 1], true), &grass_rows)
                    .faces
                    .is_empty(),
                "none".into(),
            ),
        ),
        (
            "grass_no_aabb",
            check(
                faces(&aabb_model([3, 3], false), &grass_rows)
                    .faces
                    .is_empty(),
                "none".into(),
            ),
        ),
        (
            "grass_node_offset",
            check(
                both.faces
                    .iter()
                    .all(|f| f.corners.iter().all(|c| c[0] >= 1.0 && c[1] >= 2.0)),
                format!("{:?}", both.faces.first()),
            ),
        ),
        (
            "grass_count",
            check(
                first.len() == 2 * 200,
                format!("{} of 400 (trunc(50 × 4) a face)", first.len()),
            ),
        ),
        (
            "grass_thin_face_none",
            check(thin.is_empty(), format!("{}", thin.len())),
        ),
        (
            "grass_seeded",
            check(
                first == again && first != moved,
                "same place same tufts; moved, different".into(),
            ),
        ),
        (
            "grass_variants",
            check(
                lower_left.iter().all(|t| t.variant == 0),
                format!("{}", lower_left.len()),
            ),
        ),
        (
            "grass_draws",
            check(
                first.iter().all(|t| {
                    t.position[2] == 0.0
                        && (0.8..=1.2).contains(&t.size)
                        && (0.0..std::f32::consts::TAU).contains(&t.yaw)
                        && (0.0..=1.0).contains(&t.lightmap_uv[0])
                }),
                "on the plane; size, yaw and lightmap in range".into(),
            ),
        ),
        (
            "msvc_rand_sequence",
            check(sequence == [41, 18467, 6334], format!("{sequence:?}")),
        ),
    ]
}
