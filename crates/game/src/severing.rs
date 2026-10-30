//! Dismemberment: a blade's killing blow cuts a limb off about one time in
//! three, as JA's saber cuts (`g_combat.c`'s rules, our numbers): the head,
//! the waist (the body in half), an arm, the right hand or a leg, by where
//! the blade landed. A lightsaber's, and (the owner's addition) any edged
//! melee weapon's, a sword's or a vibroblade's; never a staff's or a baton's. View-side like the ragdoll it acts on:
//! every client has the killing hit in its snapshots and draws the same cut
//! from it (a number of the character and its hit's tick, not a draw), and
//! the sim never knows.
//!
//! The ragdoll lets the limb go (`ragdoll::World::sever`), thrown off the
//! way the blade went. The scene parts the body's skinned meshes where the
//! limb hangs (`ragdoll::cut`): the stump and the limb each keep their own
//! triangles, skinned so nothing stretches, and each wound is closed by a
//! cap, lit as the body is. A lightsaber's is seared, glowing at its rim
//! whatever the light and cooling over seconds to a dark burn (on a droid,
//! hot metal to scorched), and puffs JA's `saber/saber_cut` (sparks, steam,
//! smoke); a blade's is raw flesh (on a droid, bare metal), and puffs
//! nothing.
//!
//! The marks a blade burnt on the body (`body_marks`) are parted with each
//! mesh they lie on, by the same cut: the stump keeps its half, the limb
//! takes the other.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::skinning::{SkinnedMesh as Skin, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use ragdoll::cut::{Cut, SkinnedMesh, Wound};
use ragdoll::{Frame, Material, Template};
use sim::SimId;
use sim::combat::fighter::Event;
use sim::combat::sheet::damage_types;
use sim::combat::{Delivery, Sheet};

use crate::cast::Rest;
use crate::material::OdysseyMaterial;
use crate::ragdolls::Doll;

/// How often a blade's killing blow cuts (in a hundred): tuning, the
/// owner's choice (KOTOR cuts nothing).
const CHANCE: u64 = 33;
/// How fast a severed limb leaves its body (m/s) and how hard it tumbles
/// (rad/s).
const THROW: f32 = 6.0;
const TUMBLE: f32 = 9.0;
/// The heaviest piece thrown that hard (kg); a heavier one goes as much
/// slower as it is heavier.
const LIGHT: f32 = 5.0;
/// How long a seared cut glows before it is a burn (s).
const COOLING: f32 = 3.0;
/// A lightsaber's cut puffs this (`combat_fx::Bursts`), at the joint; then
/// both its wounds smoulder, a puff of `SMOULDER` every `WISP_MS` for
/// `SMOULDER_MS` (view ms).
const PUFF: &str = "saber/saber_cut";
const SMOULDER: &str = "volumetric/black_smoke";
const WISP_MS: f64 = 150.0;
const SMOULDER_MS: f64 = 900.0;
/// The effects severing asks `combat_fx` to load.
pub const EFFECTS: [&str; 2] = [PUFF, SMOULDER];

/// A saber's wound smouldering: a point on a node (its own space, Bevy's),
/// the next wisp and the last (the view's ms).
pub struct Smoulder {
    node: Entity,
    local: Vec3,
    next_ms: f64,
    until_ms: f64,
}

/// The wounds smouldering.
#[derive(Resource, Default)]
pub struct Smoulders(Vec<Smoulder>);

/// What cut: a lightsaber, seared and smoking, or a blade's edge (a sword,
/// a vibroblade), raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Saber,
    Blade,
}

/// The edge a killing `hit` struck with, if one that cuts: a lightsaber,
/// or a melee weapon with an edge as `striker` holds it, one that slashes
/// or pierces (a sword, a vibroblade; a staff or a baton only bludgeons).
pub fn edge(hit: &Event, striker: Option<&Sheet>) -> Option<Edge> {
    let edged = damage_types::SLASHING | damage_types::PIERCING;
    match hit.weapon? {
        Delivery::Saber(_) => Some(Edge::Saber),
        Delivery::Melee => striker?
            .weapons
            .iter()
            .flatten()
            .any(|w| w.delivery == Delivery::Melee && w.damage_types & edged != 0)
            .then_some(Edge::Blade),
        _ => None,
    }
}

/// A number from a character and the tick of the hit that killed it, the
/// same on every client: which deaths cut, and how.
fn roll(id: SimId, tick: u64, salt: u64) -> u64 {
    crate::view_rng::mix64(
        u64::from(id.0)
            .wrapping_mul(crate::view_rng::GOLDEN_64)
            .wrapping_add(tick.wrapping_mul(0xD1B5_4A32_D192_ED03))
            .wrapping_add(salt),
    )
}

/// The part a killing `hit` of an edge cuts off `template`'s body, if it
/// cuts: one time in three (by the character and its killing hit).
pub fn limb(template: &Template, hit: &Event, id: SimId) -> Option<usize> {
    if roll(id, hit.tick, 1) % 100 >= CHANCE {
        return None;
    }
    let parts = &template.parts;
    if parts.iter().any(|p| !p.name.is_empty()) {
        let coin = roll(id, hit.tick, 2).is_multiple_of(2);
        let name = crate::cast::spot(hit.location).cut[usize::from(!coin)];
        return parts.iter().position(|p| p.name == name);
    }
    // Fitted: the part (not the root) whose middle is nearest the landing.
    let middles: Vec<Vec3> = parts
        .iter()
        .map(|p| p.rest.point(p.solid.centre()))
        .collect();
    let bounds = middles.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), &m| (lo.min(m), hi.max(m)),
    );
    let [x, z] = crate::cast::spot(hit.location).landing;
    let (min, max) = bounds;
    let target = Vec3::new(
        min.x + (max.x - min.x) * x,
        (min.y + max.y) * 0.5,
        min.z + (max.z - min.z) * z,
    );
    (1..parts.len()).min_by(|&a, &b| {
        middles[a]
            .distance(target)
            .total_cmp(&middles[b].distance(target))
    })
}

/// How a severed `limb` leaves its body (N·s), tumbling about the blade's
/// line (N·m·s): up and across the blow's `way`, the way the blade went
/// through it (`blade`: its motion there and its line, when its pass was
/// seen) and outward from the body's middle.
pub fn throw(
    template: &Template,
    places: &[Frame],
    (limb, way, blade): (usize, Vec3, Option<(Vec3, Vec3)>),
    (id, tick): (SimId, u64),
) -> (Vec3, Vec3) {
    // The limb and all that hangs on it.
    let parts = &template.parts;
    let below = |mut at: usize| loop {
        if at == limb {
            return true;
        }
        match parts[at].parent {
            Some(parent) => at = parent,
            None => return false,
        }
    };
    let mass: f32 = (0..parts.len())
        .filter(|&i| below(i))
        .map(|i| parts[i].mass * template.mass)
        .sum();
    let middle = places[0].point(parts[0].solid.centre());
    let at = places[limb].point(parts[limb].solid.centre());
    // Across the striker's line, and up: where it (and a camera behind it)
    // sees the limb leave at once. Thrown on along the line, a limb hides
    // behind its own body until it clears it.
    let across = |v: Vec3| (v - way * v.dot(way)).normalize_or_zero();
    let outward = across((at - middle).with_z(0.0));
    let side = if roll(id, tick, 3).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    let (swept, tumble) = match blade {
        Some((motion, line)) => (across(motion), line * side),
        None => (Vec3::ZERO, way.cross(Vec3::Z).normalize_or_zero() * side),
    };
    let off = (swept * 0.7 + outward * 0.7 + Vec3::Z * 0.8 + way * 0.25).normalize_or_zero();
    // A hand or a head flies; a leg or a body's half slumps off: past
    // `LIGHT`, the same kick whatever the weight.
    let lively = (LIGHT / mass).min(1.0);
    let inertia = mass * 0.05;
    (
        off * THROW * lively * mass,
        tumble * TUMBLE * lively * inertia,
    )
}

/// A part cut off a body (`ragdolls`): its doll, the part, what cut it, and
/// where the joint was (game space, metres), which way the limb went and
/// when (the view's ms).
pub struct Sever {
    pub doll: Arc<Doll>,
    pub part: usize,
    pub edge: Edge,
    pub at: Vec3,
    pub way: Vec3,
    pub ms: f64,
}

/// A cut the scene owes: the body's node entities, and the cut.
pub type Owed = (Vec<Entity>, Sever);

/// The cuts the scene owes.
#[derive(Resource, Default)]
pub struct Cuts(pub Vec<Owed>);

/// A wound's cap: a cut never parts one again.
#[derive(Component)]
pub struct WoundCap;

/// A seared cap cooling: since when (animation s, from its first frame).
#[derive(Component)]
pub struct Searing {
    since: Option<f32>,
}

/// A body's mesh parted, and the whole one it was: put back when the world
/// starts over in the same scene (its character alive again).
#[derive(Component)]
pub struct Parted(Handle<Mesh>);

/// A piece or cap a cut hung on a body: gone when the world starts over.
#[derive(Component)]
pub struct Piece;

/// A model a cut took with a part (a head's), and the node it hung on and
/// how: hung back when the world starts over.
#[derive(Component)]
pub struct Rehung(Entity, Transform);

/// The nodes at and under `at` in a skeleton.
fn under(rest: &Rest, at: usize) -> BTreeSet<usize> {
    let mut below = BTreeSet::from([at]);
    for node in 0..rest.len() {
        let mut up = rest[node].0;
        while let Some(parent) = up {
            if parent == at {
                below.insert(node);
                break;
            }
            up = rest[parent].0;
        }
    }
    below
}

/// A Bevy mesh of a cut piece, kept in main memory too (a later cut reads
/// it again).
fn bevy_mesh(piece: &SkinnedMesh, colours: Option<[[f32; 4]; 2]>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, piece.positions.clone());
    if piece.normals.len() == piece.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, piece.normals.clone());
    }
    if piece.uvs.len() == piece.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, piece.uvs.clone());
    }
    if let Some([middle, rim]) = colours {
        let colour: Vec<[f32; 4]> = piece
            .uvs
            .iter()
            .map(|uv| {
                let linear = LinearRgba::from_f32_array(std::array::from_fn(|i| {
                    middle[i] + (rim[i] - middle[i]) * uv[0]
                }));
                crate::scene::stored(linear.into())
                    .to_linear()
                    .to_f32_array()
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colour);
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(piece.joints.clone()),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, piece.weights.clone());
    mesh.insert_indices(Indices::U32(piece.indices.clone()));
    mesh
}

/// A skinned Bevy mesh as a cut piece; `None` if it is not one.
fn piece(mesh: &Mesh) -> Option<SkinnedMesh> {
    let floats3 = |attribute| match mesh.attribute(attribute)? {
        VertexAttributeValues::Float32x3(values) => Some(values.clone()),
        _ => None,
    };
    let joints = match mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)? {
        VertexAttributeValues::Uint16x4(values) => values.clone(),
        _ => return None,
    };
    let weights = match mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)? {
        VertexAttributeValues::Float32x4(values) => values.clone(),
        _ => return None,
    };
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(values)) => values.clone(),
        _ => Vec::new(),
    };
    let indices = match mesh.indices()? {
        Indices::U32(values) => values.clone(),
        Indices::U16(values) => values.iter().map(|&i| u32::from(i)).collect(),
    };
    Some(SkinnedMesh {
        positions: floats3(Mesh::ATTRIBUTE_POSITION)?,
        normals: floats3(Mesh::ATTRIBUTE_NORMAL).unwrap_or_else(Vec::new),
        uvs,
        joints,
        weights,
        indices,
    })
}

/// A cap's colours, its middle and its rim: a lightsaber's seared, glowing
/// as it is cut; a blade's raw.
fn wound_colours(edge: Edge, material: Material) -> [[f32; 4]; 2] {
    match (edge, material) {
        (Edge::Saber, Material::Flesh) => [[0.16, 0.03, 0.02, 1.0], [1.0, 0.42, 0.08, 1.0]],
        (Edge::Saber, Material::Metal) => [[0.10, 0.10, 0.11, 1.0], [1.0, 0.62, 0.22, 1.0]],
        (Edge::Blade, Material::Flesh) => [[0.12, 0.015, 0.015, 1.0], [0.30, 0.04, 0.035, 1.0]],
        (Edge::Blade, Material::Metal) => [[0.10, 0.10, 0.11, 1.0], [0.42, 0.42, 0.45, 1.0]],
    }
}

/// A skinned mesh of the scene: its entity, mesh, skin and material, the
/// node it hangs on, and whether a cut parted it already.
type Skinned = (
    Entity,
    &'static Mesh3d,
    &'static Skin,
    &'static MeshMaterial3d<OdysseyMaterial>,
    &'static ChildOf,
    Has<Parted>,
);

/// How far from the cut joint a wound's middle may be (metres).
const WOUND_REACH: f32 = 0.2;

/// How a piece hung on a body is drawn: in the body's material, or as a
/// cap in the body's light (seared, cooling, by a lightsaber's).
#[derive(Clone, Copy)]
enum Look {
    Body,
    Cap {
        colours: [[f32; 4]; 2],
        searing: bool,
    },
}

/// What `hang` draws with: the commands and the meshes and materials.
type Drawing<'a, 'w, 's> = (
    &'a mut Commands<'w, 's>,
    &'a mut Assets<Mesh>,
    &'a mut Assets<OdysseyMaterial>,
);

/// Hangs `piece`, skinned by `skin`, on the node entity `on`, by the body
/// material `body`.
fn hang(
    (commands, meshes, materials): Drawing,
    (piece, skin, on): (&SkinnedMesh, &Skin, Entity),
    body: &MeshMaterial3d<OdysseyMaterial>,
    look: Look,
) {
    if piece.indices.is_empty() {
        return;
    }
    let (colours, material) = match look {
        Look::Body => (None, body.0.clone()),
        Look::Cap { colours, searing } => {
            let Some(body) = materials.get(&body.0) else {
                return;
            };
            let mut cap = body.clone().bare();
            if searing {
                cap.params.self_illum = Vec4::ONE;
            }
            (Some(colours), materials.add(cap))
        }
    };
    let mut spawned = commands.spawn((
        Mesh3d(meshes.add(bevy_mesh(piece, colours))),
        skin.clone(),
        Transform::IDENTITY,
        bevy::camera::visibility::NoFrustumCulling,
        Piece,
        MeshMaterial3d(material),
    ));
    match look {
        Look::Body => {}
        Look::Cap { searing, .. } => {
            spawned.insert(WoundCap);
            if searing {
                spawned.insert(Searing { since: None });
            }
        }
    }
    let child = spawned.id();
    commands.entity(on).add_child(child);
}

/// What `close_model` reads of the scene: its hierarchy, its skinned
/// meshes, where everything is and the skins' bind poses.
type Scene<'a, 'w, 's> = (
    &'a Query<'w, 's, &'static Children>,
    &'a Query<'w, 's, Skinned, Without<WoundCap>>,
    &'a Query<'w, 's, (&'static GlobalTransform, &'static Transform)>,
    &'a Assets<SkinnedMeshInverseBindposes>,
);

/// The bodies' marks (`body_marks`): each piece's mesh, stage and mark, and
/// whether it is the glow stage.
type Marks<'w, 's> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        &'static MeshMaterial3d<crate::glow_shell::GlowMaterial>,
        &'static crate::body_marks::Mark,
        Has<crate::body_marks::Glowing>,
    ),
>;

/// The scene's node hierarchy, where everything is (last frame) and on its
/// parent, and the skins' bind poses.
type Places<'w, 's> = (
    Query<'w, 's, &'static Children>,
    Query<'w, 's, (&'static GlobalTransform, &'static Transform)>,
    Res<'w, Assets<SkinnedMeshInverseBindposes>>,
);

/// Makes the cuts owed: each skinned mesh of the body is parted where it
/// has skin on both sides (the stump's piece in its place, the limb's
/// beside it), and every wound at the joint, on either side, closed by a
/// cap; a lightsaber's cut puffs.
pub fn apply(
    (mut cuts, mut bursts, mut smoulders): (
        ResMut<Cuts>,
        Option<ResMut<crate::combat_fx::Bursts>>,
        ResMut<Smoulders>,
    ),
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<OdysseyMaterial>>),
    skinned: Query<Skinned, Without<WoundCap>>,
    (children, globals, binds): Places,
    (pieces, marks): (Query<(), With<Piece>>, Marks),
    (mut made, mut facts): (Local<(usize, usize)>, ResMut<console::DumpFacts>),
    mut commands: Commands,
) {
    for (nodes, sever) in cuts.0.drain(..) {
        made.0 += 1;
        let (template, rest) = (&sever.doll.template, &sever.doll.rest);
        let part = &template.parts[sever.part];
        if let (Edge::Saber, Some(bursts)) = (sever.edge, bursts.as_deref_mut()) {
            bursts.0.push(crate::combat_fx::Burst {
                effect: PUFF,
                at: sever.at / collision::UNIT,
                normal: sever.way,
                ms: sever.ms as f32,
                times: 1,
                axis: None,
            });
        }
        let stump_node = part.parent.map(|p| template.parts[p].node);
        let gone = under(rest, part.node);
        let model = crate::ragdolls::model_space(rest, |_, _, rest| rest);
        let cap = Look::Cap {
            colours: wound_colours(sever.edge, template.material),
            searing: sever.edge == Edge::Saber,
        };
        for (entity, mesh3d, skin, material, parent, parted) in &skinned {
            // A body's meshes are skinned to its own nodes.
            let node_of = |joint: &Entity| nodes.iter().position(|n| n == joint);
            let slots: Vec<Option<usize>> = skin.joints.iter().map(node_of).collect();
            let Some(hung) =
                node_of(&parent.parent()).filter(|_| slots.iter().any(Option::is_some))
            else {
                continue;
            };
            let Some(whole) = meshes.get(&mesh3d.0).and_then(piece) else {
                continue;
            };
            let on_limb = |slot: u16| {
                slots
                    .get(usize::from(slot))
                    .copied()
                    .flatten()
                    .is_some_and(|n| gone.contains(&n))
            };
            let count = slots.len() as u16;
            // The slot of the node nearest above or at `node` the skin has.
            let slot_at = |mut node: Option<usize>| {
                while let Some(n) = node {
                    if let Some(slot) = slots.iter().position(|&s| s == Some(n)) {
                        return Some(slot as u16);
                    }
                    node = rest[n].0;
                }
                None
            };
            // The stump's: the node nearest above the cut the skin has, or
            // (a neck skinned beside the chest) the one it blends into most.
            let stump = slot_at(stump_node)
                .filter(|&slot| !on_limb(slot))
                .or_else(|| whole.blended(&on_limb));
            // The limb's: its own node's, or the limb's node nearest it.
            let depth = |slot: u16| {
                let mut up = slots[usize::from(slot)].and_then(|n| rest[n].0);
                std::iter::from_fn(|| {
                    let at = up?;
                    up = rest[at].0;
                    Some(())
                })
                .count()
            };
            let root = slot_at(Some(part.node))
                .filter(|&slot| on_limb(slot))
                .or_else(|| {
                    (0..count)
                        .filter(|&slot| on_limb(slot))
                        .min_by_key(|&slot| depth(slot))
                });
            // The joint in the mesh's own space.
            let at = model[hung]
                .inverse()
                .point(model[part.node].translation)
                .to_array();
            let wound = |slot| Wound {
                at,
                within: WOUND_REACH,
                slot,
            };
            match (stump, root) {
                // Skin on both sides: parted.
                (Some(stump), Some(root)) => {
                    let cut = Cut {
                        limb: &on_limb,
                        stump,
                        root,
                    };
                    let (stays, goes) = whole.cut(&cut);
                    // Its marks (`body_marks`) parted with it, skinned as it
                    // is: else a mark across the cut stretches from the
                    // body to the limb. A glow streak the cut passes through
                    // goes out: the caps glow as the cut sears, and the
                    // streak's flicker over them lit the wound for seconds.
                    for &child in children.get(entity).into_iter().flatten() {
                        let Ok((mark_mesh, stage, &mark, glowing)) = marks.get(child) else {
                            continue;
                        };
                        let Some(mark_whole) = meshes.get(&mark_mesh.0).and_then(piece) else {
                            continue;
                        };
                        let (mark_stays, mark_goes) = mark_whole.cut(&cut);
                        made.1 += 1;
                        let across =
                            !mark_stays.indices.is_empty() && !mark_goes.indices.is_empty();
                        if glowing && across {
                            commands.entity(child).despawn();
                            continue;
                        }
                        if mark_stays.indices.is_empty() {
                            commands.entity(child).despawn();
                        } else {
                            let kept = meshes.add(bevy_mesh(&mark_stays, None));
                            commands.entity(child).insert(Mesh3d(kept));
                        }
                        if !mark_goes.indices.is_empty() {
                            let gone = commands
                                .spawn((
                                    Mesh3d(meshes.add(bevy_mesh(&mark_goes, None))),
                                    stage.clone(),
                                    mark,
                                    skin.clone(),
                                    Transform::IDENTITY,
                                    bevy::camera::visibility::NoFrustumCulling,
                                    Piece,
                                ))
                                .id();
                            commands.entity(parent.parent()).add_child(gone);
                        }
                    }
                    let on = (&goes, skin, parent.parent());
                    let drawing = (&mut commands, &mut *meshes, &mut *materials);
                    hang(drawing, on, material, Look::Body);
                    let wounds = [(stays.caps(&wound(stump))), goes.caps(&wound(root))];
                    for wound in &wounds {
                        hang(
                            (&mut commands, &mut *meshes, &mut *materials),
                            (wound, skin, parent.parent()),
                            material,
                            cap,
                        );
                    }
                    let mut body = commands.entity(entity);
                    if !parted {
                        body.insert(Parted(mesh3d.0.clone()));
                    }
                    body.insert(Mesh3d(meshes.add(bevy_mesh(&stays, None))));
                }
                // All the limb's, it goes with it as it is; all the body's,
                // it stays: only its wound closed.
                (None, Some(slot)) | (Some(slot), None) => {
                    let wound = whole.caps(&wound(slot));
                    hang(
                        (&mut commands, &mut *meshes, &mut *materials),
                        (&wound, skin, parent.parent()),
                        material,
                        cap,
                    );
                }
                (None, None) => {}
            }
        }
        // A model hung on a node outside the cut part but its own (a head's
        // model at `headhook`, which KOTOR hangs off the torso) goes with
        // the part, its own wound closed: a node is the part's nearest it
        // at rest.
        let cut_node = nodes[part.node];
        let nearest = |node: usize| {
            let at = model[node].translation;
            template.parts.iter().min_by(|a, b| {
                let off = |p: &ragdoll::Part| model[p.node].translation.distance(at);
                off(a).total_cmp(&off(b))
            })
        };
        let only_the_cut = |hook: usize| {
            !gone.contains(&hook) && nearest(hook).is_some_and(|p| gone.contains(&p.node))
        };
        let Ok((onto, _)) = globals.get(cut_node) else {
            continue;
        };
        let joint = onto.translation();
        if sever.edge == Edge::Saber {
            // The stump's wound where the joint was on it; the limb's at
            // its own root.
            let stump = stump_node
                .and_then(|node| Some((nodes[node], globals.get(nodes[node]).ok()?.0)))
                .map(|(node, at)| (node, at.affine().inverse().transform_point3(joint)));
            let wounds = stump.into_iter().chain([(cut_node, Vec3::ZERO)]);
            smoulders.0.extend(wounds.map(|(node, local)| Smoulder {
                node,
                local,
                next_ms: sever.ms,
                until_ms: sever.ms + SMOULDER_MS,
            }));
        }
        for hook in (0..nodes.len()).filter(|&hook| only_the_cut(hook)) {
            let models = children.get(nodes[hook]).into_iter().flatten();
            // Hooked models: neither the body's nodes, its meshes nor pieces.
            let hooked =
                |&&m: &&Entity| !nodes.contains(&m) && !pieces.contains(m) && !skinned.contains(m);
            for &model in models.filter(hooked) {
                let Ok((was, &hung)) = globals.get(model) else {
                    continue;
                };
                let kept = Transform::from_matrix(onto.to_matrix().inverse() * was.to_matrix());
                commands
                    .entity(model)
                    .insert((ChildOf(cut_node), kept, Rehung(nodes[hook], hung)));
                close_model(
                    model,
                    joint,
                    (&children, &skinned, &globals, &binds),
                    (&mut commands, &mut *meshes, &mut *materials),
                    cap,
                );
            }
        }
    }
    facts.0.insert(
        "severing".into(),
        serde_json::json!({ "cuts": made.0, "marks_parted": made.1 }),
    );
}

/// Closes the wounds of a model gone with a cut part: each of its skinned
/// meshes' open loops near `joint` (game's world, as last drawn).
fn close_model(
    model: Entity,
    joint: Vec3,
    (children, skinned, globals, binds): Scene<'_, '_, '_>,
    (commands, meshes, materials): Drawing,
    cap: Look,
) {
    let mut below = vec![model];
    while let Some(at) = below.pop() {
        below.extend(children.get(at).into_iter().flatten().copied());
        let Ok((_, mesh3d, skin, material, parent, _)) = skinned.get(at) else {
            continue;
        };
        let Some(whole) = meshes.get(&mesh3d.0).and_then(piece) else {
            continue;
        };
        // The joint in the mesh's space, by the bone that moves it most.
        let mut shares = vec![0.0f32; skin.joints.len()];
        for (joints, weights) in whole.joints.iter().zip(&whole.weights) {
            for (&j, &w) in joints.iter().zip(weights) {
                if let Some(share) = shares.get_mut(usize::from(j)) {
                    *share += w;
                }
            }
        }
        let Some(slot) = (0..shares.len()).max_by(|&a, &b| shares[a].total_cmp(&shares[b])) else {
            continue;
        };
        let (Some(bind), Ok((bone, _))) = (
            binds.get(&skin.inverse_bindposes).and_then(|b| b.get(slot)),
            globals.get(skin.joints[slot]),
        ) else {
            continue;
        };
        let at = (bone.to_matrix() * *bind).inverse().transform_point3(joint);
        let wound = whole.caps(&Wound {
            at: at.to_array(),
            within: WOUND_REACH,
            slot: slot as u16,
        });
        hang(
            (&mut *commands, &mut *meshes, &mut *materials),
            (&wound, skin, parent.parent()),
            material,
            cap,
        );
    }
}

/// The smouldering wounds' wisps up to the view's moment, each where its
/// wound is now (`combat_fx` plays them).
pub fn smoulder(
    presented: Res<crate::sim_host::PresentedSnapshot>,
    mut smoulders: ResMut<Smoulders>,
    bursts: Option<ResMut<crate::combat_fx::Bursts>>,
    globals: Query<&GlobalTransform>,
) {
    let Some(mut bursts) = bursts else {
        return;
    };
    let now = crate::sim_host::view_ms(&presented.0);
    let to_game = crate::scene::game_to_bevy().inverse();
    smoulders.0.retain_mut(|wound| {
        let Ok(node) = globals.get(wound.node) else {
            return false;
        };
        let at = to_game * node.transform_point(wound.local) / collision::UNIT;
        while wound.next_ms <= now.min(wound.until_ms) {
            bursts.0.push(crate::combat_fx::Burst {
                effect: SMOULDER,
                at,
                normal: Vec3::Z,
                ms: wound.next_ms as f32,
                times: 2,
                axis: None,
            });
            wound.next_ms += WISP_MS;
        }
        wound.next_ms <= wound.until_ms
    });
}

/// Cools the seared caps: their glow, which no light needs, fades to a burn
/// the light shows over `COOLING`, then stays.
pub fn cool(
    time: Res<Time>,
    held: Res<console::AnimationTime>,
    mut caps: ResMut<Assets<OdysseyMaterial>>,
    mut searing: Query<(Entity, &mut Searing, &MeshMaterial3d<OdysseyMaterial>)>,
    mut commands: Commands,
) {
    let now = crate::scene::animation_seconds(&time, &held);
    for (entity, mut searing, material) in &mut searing {
        let since = *searing.since.get_or_insert(now);
        let cooled = ((now - since) / COOLING).clamp(0.0, 1.0);
        if let Some(mut material) = caps.get_mut(&material.0) {
            // Fast at first, as a cut stops glowing.
            let glow = 1.0 - 0.75 * cooled * (2.0 - cooled);
            material.params.tint = Vec4::new(glow, glow * (1.0 - 0.3 * cooled), glow, 1.0);
            let heat = (1.0 - cooled) * (1.0 - cooled);
            material.params.self_illum = Vec4::new(heat, heat, heat, 1.0);
        }
        if cooled >= 1.0 {
            commands.entity(entity).remove::<Searing>();
        }
    }
}

/// When the world starts over (`SceneSwap`), the cuts made in it are undone:
/// the cuts owed dropped, each parted body's whole mesh put back, its pieces
/// and caps gone. A new scene's bodies are new; this is for a world started
/// over in the same one.
pub fn uncut(
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    mut seen: Local<Option<u32>>,
    (mut cuts, mut smoulders): (ResMut<Cuts>, ResMut<Smoulders>),
    parted: Query<(Entity, &Parted)>,
    pieces: Query<Entity, With<Piece>>,
    rehung: Query<(Entity, &Rehung)>,
    mut commands: Commands,
) {
    let generation = swap.map(|swap| swap.generation);
    if *seen == generation {
        return;
    }
    *seen = generation;
    cuts.0.clear();
    smoulders.0.clear();
    for (entity, Parted(whole)) in &parted {
        commands
            .entity(entity)
            .insert(Mesh3d(whole.clone()))
            .remove::<Parted>();
    }
    for piece in &pieces {
        commands.entity(piece).despawn();
    }
    for (model, &Rehung(hook, hung)) in &rehung {
        commands
            .entity(model)
            .insert((ChildOf(hook), hung))
            .remove::<Rehung>();
    }
}
