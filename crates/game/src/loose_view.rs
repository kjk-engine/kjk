//! What the viewers (`kjk view`, `kjk rig`) draw to show a body's loose
//! parts, for https://github.com/kjk-engine/roadmap/issues/5.
//!
//! - **Capsules**: the 14 ragdoll capsules (`ragdoll::Template::fit`) as
//!   wire frames, each on its node, so they move with the animated
//!   skeleton every frame.
//! - **Ghost**: the body's surfaces drawn faint, so the capsules and the
//!   skeleton show through a robe.
//! - **Marks**: a dot on every garment vertex that starts inside a capsule
//!   in the rest pose, drawn where the animation has moved that vertex. A
//!   garment vertex is one that a loose bone (`asset_ja::loose`) moves most,
//!   or one of a KOTOR 1 Jedi robe's legs mesh, which holds the whole robe.
//! - **Bones**: the points that `asset_ja::loose` finds, each joined to its
//!   parent by a line in a colour for the kind of garment, and the links
//!   between neighbouring chains as thin grey lines.
//! - **Orbit camera**: the camera frames the body, follows it along the
//!   floor, turns about it with the left mouse button and zooms with the
//!   wheel. A `camera` command takes the camera from it.
//! - **Slow motion**: the model's clip plays at a fraction of its speed.

use asset_ja::loose::{self, Kind};
use asset_odyssey::anim::AnimationIr;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use console::{CameraRequest, DumpDue, DumpFacts};
use ragdoll::{Group, Solid, Template};

use crate::lifecycle_view::SceneCamera;
use crate::material::OdysseyMaterial;
use crate::scene::{FlyCamera, SceneContent, SceneModel, camera_transform, game_to_bevy, to_mat4};
use crate::sim_blades::Quads;
use console::CameraPose;

/// What a viewer draws over the first model of its scene.
#[derive(Debug, Clone)]
pub struct Loose {
    pub capsules: bool,
    pub marks: bool,
    pub bones: bool,
    /// Run the robe and cape solver on the body's loose bones
    /// (`loose_sim`).
    pub sim: bool,
    /// How opaque the body's surfaces are drawn, 0 to 1.
    pub ghost: Option<f32>,
    /// The share of its speed that the model's clip plays at.
    pub slow: f32,
    /// The camera's first angle about the body, degrees from the front.
    pub angle: f32,
    /// How many times nearer than the framing the camera starts.
    pub zoom: f32,
    /// With `sim`, every clip of the body's supermodel chain. They set how
    /// far each point may stray, as in the game (`loose_parts`).
    pub chain: Vec<AnimationIr>,
}

impl Default for Loose {
    fn default() -> Loose {
        Loose {
            capsules: false,
            marks: false,
            bones: false,
            sim: false,
            ghost: None,
            slow: 1.0,
            angle: 0.0,
            zoom: 1.0,
            chain: Vec::new(),
        }
    }
}

/// The speed the model's clip plays at, as a share of its own (1 is as it
/// was made; `animate` divides the animation time by it).
#[derive(Resource, Debug, Clone, Copy)]
pub struct Slow(pub f32);

/// The orbit camera: where it looks (game space), how far it stands, and
/// the angles it stands at about that point.
#[derive(Resource, Debug)]
pub(crate) struct Orbit {
    /// The body's height the camera looks at.
    height: f32,
    distance: f32,
    /// Degrees from +X, turning toward +Y.
    yaw: f32,
    pitch: f32,
    /// A `camera` command took the camera.
    given_up: bool,
}

/// A body whose loose parts are drawn.
#[derive(Component)]
pub(crate) struct Drawn {
    capsules: Option<Template>,
    /// The loose trees and links in the rest pose.
    rig: loose::Rig,
    marks: Vec<Mark>,
    stats: Stats,
    draw_capsules: bool,
    draw_marks: bool,
    draw_bones: bool,
    /// The node the camera follows along the floor.
    follow: usize,
}

/// A body whose surfaces were drawn faint.
#[derive(Component)]
struct Ghosted;

/// A garment vertex that starts inside a capsule: its place in its mesh
/// and the nodes that move it.
struct Mark {
    position: Vec3,
    joints: [(usize, Mat4, f32); 4],
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    garment: usize,
    inside: usize,
    inside_legs: usize,
    deepest: f32,
}

fn kind_colour(kind: Kind) -> [f32; 4] {
    match kind {
        Kind::Cape => [1.0, 0.45, 0.1, 1.0],
        Kind::RobeSkirt => [0.1, 0.9, 1.0, 1.0],
        Kind::Flap => [0.7, 1.0, 0.2, 1.0],
        Kind::Hood => [1.0, 0.2, 0.9, 1.0],
        Kind::Sleeve => [0.6, 0.5, 1.0, 1.0],
        Kind::Other => [1.0, 1.0, 1.0, 1.0],
    }
}

/// Whether a mesh of `model` is a robe as a whole: the legs mesh of KOTOR 1's
/// Jedi robe bodies (`pmbj…`, `pfbj…`).
fn robe_mesh(model: &str, node: &str) -> bool {
    let model = model.to_ascii_lowercase();
    (model.starts_with("pmbj") || model.starts_with("pfbj")) && node.eq_ignore_ascii_case("legs")
}

fn group_colour(group: Group) -> [f32; 4] {
    match group {
        Group::Core => [0.95, 0.95, 0.95, 1.0],
        Group::Head => [1.0, 0.5, 0.9, 1.0],
        Group::ArmLeft => [0.3, 0.6, 1.0, 1.0],
        Group::ArmRight => [0.2, 0.95, 0.95, 1.0],
        Group::LegLeft => [1.0, 0.55, 0.1, 1.0],
        Group::LegRight => [1.0, 0.9, 0.2, 1.0],
    }
}

/// How much a ghost body's colours are brightened: faint, they would sink
/// into the black behind them.
const BRIGHTEN: f32 = 2.2;
const MARK: [f32; 4] = [1.0, 0.1, 0.1, 1.0];
const CAPSULE_LINE: f32 = 0.006;
const BONE_LINE: f32 = 0.012;
const LINK_LINE: f32 = 0.004;
const LINK: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
const MARK_SIZE: f32 = 0.02;
const TIP_SIZE: f32 = 0.012;
const RING: usize = 20;
const ARC: usize = 8;

/// A model skinned at `nodes` (each node's matrix in the space the result
/// is in): where the vertex at `position` is.
fn skinned(
    position: Vec3,
    joints: &[(usize, Mat4, f32); 4],
    nodes: impl Fn(usize) -> Mat4,
) -> Vec3 {
    let total: f32 = joints.iter().map(|&(_, _, weight)| weight).sum();
    if total <= 0.0 {
        return position;
    }
    joints
        .iter()
        .filter(|&&(_, _, weight)| weight > 0.0)
        .map(|&(node, bind, weight)| {
            nodes(node).transform_point3(bind.transform_point3(position)) * (weight / total)
        })
        .sum()
}

/// How far `p` is from the segment `a` to `b`.
fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let along = b - a;
    let length = along.length_squared();
    let t = if length > 0.0 {
        ((p - a).dot(along) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + along * t)
}

/// Each body that has a scene's first model gets its parts worked out once.
fn start(
    mut commands: Commands,
    scene: Option<Res<SceneContent>>,
    added: Query<(Entity, &SceneModel), Added<SceneModel>>,
) {
    let Some(scene) = scene else { return };
    let Some(loose) = &scene.0.loose else { return };
    for (entity, model) in &added {
        if model.index != 0 {
            continue;
        }
        let placed = &scene.0.models[0];
        let ir = &placed.ir;
        commands.insert_resource(Slow(loose.slow));
        let fitted = (loose.capsules || loose.marks)
            .then(|| {
                crate::ragdolls::fitted(ir, &crate::cast::rest_of(ir), |skeleton| {
                    Template::fit(&ragdoll::humanoid::PLAN, skeleton)
                })
            })
            .and_then(|fitted| {
                fitted
                    .inspect_err(|why| warn!("{}: no capsules: {why}", ir.name))
                    .ok()
            });
        // The view draws the points, which the clips do not move, so the
        // model's own clips are enough.
        let own: Vec<_> = ir.animations.iter().collect();
        let rig = loose::rig(ir, &own);
        if loose.bones {
            info!(
                "{}: {} loose trees, {} points, {} links\n{}",
                ir.name,
                rig.trees.len(),
                rig.trees
                    .iter()
                    .map(|tree| tree.points.len())
                    .sum::<usize>(),
                rig.links.len(),
                rig.describe(ir)
            );
        }
        let (marks, stats) = match (&fitted, loose.marks) {
            (Some(template), true) => garment_marks(ir, template, &rig),
            _ => (Vec::new(), Stats::default()),
        };
        if loose.marks {
            info!(
                "{}: {} garment vertices, {} start inside a capsule ({} inside a thigh or shin), deepest {:.1} cm",
                ir.name,
                stats.garment,
                stats.inside,
                stats.inside_legs,
                stats.deepest * 100.0
            );
        }
        let follow = ir.node_index(asset_odyssey::hooks::PELVIS).unwrap_or(0);
        let (min, max) = ir.nodes.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), node| {
                let at = Vec3::new(node.world[0][3], node.world[1][3], node.world[2][3]);
                (min.min(at), max.max(at))
            },
        );
        let height = (max.z + min.z) / 2.0;
        let extent = (max.z - min.z).max(1.0);
        // A cutscene's stunt clip turns the whole body with its
        // `cutscenedummy` (Malak's in `m44aa_c02` by 172 degrees), so the
        // front is where the dummy turns the model's +Y at the clip's start.
        let front = placed
            .clip
            .as_ref()
            .and_then(|clip| {
                let dummy = ir.node_index(asset_odyssey::hooks::CUTSCENE_DUMMY)?;
                let at = asset_odyssey::anim::posed(
                    ir,
                    (clip, &clip.bind(ir)),
                    ir.animation_scale,
                    dummy,
                    0.0,
                );
                Some(at[1][1].atan2(at[0][1]).to_degrees())
            })
            .unwrap_or(FRONT);
        commands.insert_resource(Orbit {
            height,
            distance: extent * 1.2 / loose.zoom.max(0.1),
            yaw: front + loose.angle,
            pitch: 8.0,
            given_up: false,
        });
        commands.entity(entity).insert(Drawn {
            capsules: fitted.filter(|_| loose.capsules),
            rig,
            marks,
            stats,
            draw_capsules: loose.capsules,
            draw_marks: loose.marks,
            draw_bones: loose.bones,
            follow,
        });
    }
}

/// The garment vertices of `ir` that start inside a capsule of `template`,
/// and how many there are of each kind.
fn garment_marks(
    ir: &asset_odyssey::model::ModelIr,
    template: &Template,
    rig: &loose::Rig,
) -> (Vec<Mark>, Stats) {
    let loose_nodes: std::collections::BTreeSet<usize> = rig
        .trees
        .iter()
        .flat_map(|tree| tree.points.iter().map(|point| point.node))
        .collect();
    let rest: Vec<Mat4> = ir.nodes.iter().map(|node| to_mat4(&node.world)).collect();
    let capsules: Vec<(usize, [Vec3; 2], f32, bool)> = template
        .parts
        .iter()
        .filter_map(|part| match part.solid {
            Solid::Capsule { ends, radius } => Some((
                part.node,
                ends.map(|end| part.rest.point(end)),
                radius,
                part.name.contains("thigh") || part.name.contains("shin"),
            )),
            Solid::Cuboid { .. } => None,
        })
        .collect();
    let mut marks = Vec::new();
    let mut stats = Stats::default();
    for part in &ir.meshes {
        let Some(skin) = &part.skin else { continue };
        let whole = robe_mesh(&ir.name, &ir.nodes[part.node].name);
        let binds: Vec<Mat4> = skin.inverse_binds.iter().map(to_mat4).collect();
        for (vertex, position) in part.positions.iter().enumerate() {
            let (joints, weights) = (skin.vertex_joints[vertex], skin.vertex_weights[vertex]);
            let slots = [0, 1, 2, 3].map(|slot| {
                let joint = usize::from(joints[slot]);
                match (skin.joints.get(joint), binds.get(joint)) {
                    (Some(&node), Some(&bind)) => (node, bind, weights[slot]),
                    _ => (0, Mat4::IDENTITY, 0.0),
                }
            });
            let main = slots
                .iter()
                .max_by(|a, b| a.2.total_cmp(&b.2))
                .map_or(0, |slot| slot.0);
            if !whole && !loose_nodes.contains(&main) {
                continue;
            }
            stats.garment += 1;
            let position = Vec3::from_array(*position);
            let at = skinned(position, &slots, |node| rest[node]);
            let mut deepest = 0.0_f32;
            let mut in_leg = false;
            for &(_, [a, b], radius, leg) in &capsules {
                let depth = radius - distance_to_segment(at, a, b);
                deepest = deepest.max(depth);
                in_leg |= leg && depth > 0.0;
            }
            stats.inside_legs += usize::from(in_leg);
            if deepest > 0.0 {
                stats.inside += 1;
                stats.deepest = stats.deepest.max(deepest);
                marks.push(Mark {
                    position,
                    joints: slots,
                });
            }
        }
    }
    (marks, stats)
}

/// The body's surfaces drawn faint, once its meshes exist.
fn ghost(
    mut commands: Commands,
    scene: Option<Res<SceneContent>>,
    bodies: Query<(Entity, &SceneModel), Without<Ghosted>>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    let Some(alpha) = scene.and_then(|scene| scene.0.loose.as_ref()?.ghost) else {
        return;
    };
    for (entity, model) in &bodies {
        if model.index != 0 {
            continue;
        }
        for descendant in children.iter_descendants(entity) {
            let Ok(mut worn) = meshes.get_mut(descendant) else {
                continue;
            };
            let Some(mut copy) = materials.get(&worn.0).map(crate::going_view::blended) else {
                continue;
            };
            copy.params.tint.w *= alpha;
            copy.params.tint *= Vec4::new(BRIGHTEN, BRIGHTEN, BRIGHTEN, 1.0);
            worn.0 = materials.add(copy);
        }
        commands.entity(entity).insert(Ghosted);
    }
}

/// The overlay's mesh, rebuilt every frame.
#[derive(Resource, Default)]
struct Overlay {
    mesh: Option<Handle<Mesh>>,
}

/// A wire capsule from `a` to `b` of `radius`: three rings, four lines along
/// it and two half circles at each end.
fn capsule(quads: &mut Quads, eye: Vec3, (a, b): (Vec3, Vec3), radius: f32, colour: [f32; 4]) {
    let along = b - a;
    let direction = along.try_normalize().unwrap_or(Vec3::Y);
    let (u, v) = direction.any_orthonormal_pair();
    let around = |angle: f32| (u * angle.cos() + v * angle.sin()) * radius;
    for centre in [a, (a + b) / 2.0, b] {
        for step in 0..RING {
            let from = step as f32 / RING as f32 * std::f32::consts::TAU;
            let to = (step + 1) as f32 / RING as f32 * std::f32::consts::TAU;
            quads.line(
                eye,
                centre + around(from),
                centre + around(to),
                CAPSULE_LINE,
                colour,
            );
        }
    }
    for quarter in 0..4 {
        let side = around(quarter as f32 * std::f32::consts::FRAC_PI_2);
        quads.line(eye, a + side, b + side, CAPSULE_LINE, colour);
    }
    for (centre, outward) in [(a, -direction), (b, direction)] {
        for across in [u, v] {
            for step in 0..ARC {
                let point = |step: usize| {
                    let angle = step as f32 / ARC as f32 * std::f32::consts::PI;
                    centre + (across * angle.cos() + outward * angle.sin()) * radius
                };
                quads.line(eye, point(step), point(step + 1), CAPSULE_LINE, colour);
            }
        }
    }
}

/// The capsules, bones and marks of every drawn body, as lines in one mesh.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    bodies: Query<(&Drawn, &SceneModel)>,
    globals: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, SceneCamera>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut overlay: ResMut<Overlay>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let eye = camera.translation();
    let right = camera.right().as_vec3();
    let mut quads = Quads::default();
    for (drawn, model) in &bodies {
        let node = |index: usize| {
            model
                .nodes
                .get(index)
                .and_then(|&entity| globals.get(entity).ok())
                .map_or(Mat4::IDENTITY, GlobalTransform::to_matrix)
        };
        if drawn.draw_capsules
            && let Some(template) = &drawn.capsules
        {
            for part in &template.parts {
                if let Solid::Capsule { ends, radius } = part.solid {
                    let place = node(part.node);
                    capsule(
                        &mut quads,
                        eye,
                        (
                            place.transform_point3(ends[0]),
                            place.transform_point3(ends[1]),
                        ),
                        radius,
                        group_colour(part.group),
                    );
                }
            }
        }
        if drawn.draw_bones {
            let at = |point: &loose::Point| {
                let offset = if point.tip {
                    Vec3::from(point.offset)
                } else {
                    Vec3::ZERO
                };
                node(point.node).transform_point3(offset)
            };
            for tree in &drawn.rig.trees {
                let colour = kind_colour(tree.kind);
                for point in &tree.points {
                    let here = at(point);
                    let from = match point.parent {
                        Some(parent) => at(&tree.points[parent]),
                        None => node(tree.attach).transform_point3(Vec3::ZERO),
                    };
                    quads.line(eye, from, here, BONE_LINE, colour);
                    let size = if point.tip { TIP_SIZE } else { MARK_SIZE };
                    quads.line(
                        eye,
                        here - right * size / 2.0,
                        here + right * size / 2.0,
                        size,
                        colour,
                    );
                }
            }
            for link in &drawn.rig.links {
                let end = |(tree, point): (usize, usize)| at(&drawn.rig.trees[tree].points[point]);
                quads.line(eye, end(link.a), end(link.b), LINK_LINE, LINK);
            }
        }
        if drawn.draw_marks {
            for mark in &drawn.marks {
                let at = skinned(mark.position, &mark.joints, node);
                quads.line(eye, at - right * 0.003, at + right * 0.003, MARK_SIZE, MARK);
            }
        }
        if dump_due.0 {
            facts.0.insert(
                "loose_view".into(),
                serde_json::json!({
                    "garment_vertices": drawn.stats.garment,
                    "inside_a_capsule": drawn.stats.inside,
                    "inside_a_thigh_or_shin": drawn.stats.inside_legs,
                    "deepest_cm": drawn.stats.deepest * 100.0,
                    "capsules": drawn.capsules.as_ref().map(|t| t.parts.len()),
                    "loose_trees": drawn.rig.trees.len(),
                    "loose_points": drawn.rig.trees.iter().map(|tree| tree.points.len()).sum::<usize>(),
                    "loose_links": drawn.rig.links.len(),
                }),
            );
        }
    }
    let mesh = quads.mesh();
    match &overlay.mesh {
        Some(handle) => {
            if let Some(mut old) = meshes.get_mut(handle) {
                *old = mesh;
            }
        }
        None => {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                NoFrustumCulling,
                NotShadowCaster,
            ));
            overlay.mesh = Some(handle);
        }
    }
}

/// Where the camera stands to see the front of a KOTOR model, degrees from +X:
/// models face +Y.
const FRONT: f32 = 90.0;

/// How far a pixel of mouse motion turns the orbit camera (degrees).
const TURN: f32 = 0.3;
/// How much each wheel step changes the camera's distance.
const ZOOM: f32 = 0.9;

/// The camera framing the body, following it along the floor, turned about
/// it by the left mouse button and zoomed by the wheel, until a `camera`
/// command takes it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn orbit(
    orbit: Option<ResMut<Orbit>>,
    request: Res<CameraRequest>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    bodies: Query<(&Drawn, &SceneModel)>,
    globals: Query<&GlobalTransform>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let Some(mut orbit) = orbit else { return };
    if request.0.is_some() {
        orbit.given_up = true;
    }
    if orbit.given_up {
        return;
    }
    if let (Some(buttons), Some(motion)) = (&buttons, &motion)
        && buttons.pressed(MouseButton::Left)
    {
        orbit.yaw -= motion.delta.x * TURN;
        orbit.pitch = (orbit.pitch + motion.delta.y * TURN).clamp(-85.0, 85.0);
    }
    if let Some(scroll) = &scroll
        && scroll.delta.y != 0.0
    {
        orbit.distance = (orbit.distance * ZOOM.powf(scroll.delta.y)).clamp(0.3, 60.0);
    }
    let Some((drawn, model)) = bodies.iter().next() else {
        return;
    };
    let followed = model
        .nodes
        .get(drawn.follow)
        .and_then(|&entity| globals.get(entity).ok())
        .map_or(Vec3::ZERO, GlobalTransform::translation);
    let followed = game_to_bevy().inverse() * followed;
    let target = followed.with_z(followed.z.max(orbit.height * 0.5));
    let (yaw, pitch) = (orbit.yaw.to_radians(), orbit.pitch.to_radians());
    let eye = target
        + Vec3::new(
            yaw.cos() * pitch.cos(),
            yaw.sin() * pitch.cos(),
            pitch.sin(),
        ) * orbit.distance;
    for (mut transform, mut fly) in &mut cameras {
        fly.yaw = orbit.yaw + 180.0;
        fly.pitch = -orbit.pitch;
        *transform = camera_transform(&CameraPose {
            position: eye.to_array(),
            yaw: fly.yaw,
            pitch: fly.pitch,
            fov: None,
        });
    }
}

pub struct LooseViewPlugin;

impl Plugin for LooseViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::loose_sim::LooseSimPlugin);
        app.init_resource::<Overlay>().add_systems(
            PostUpdate,
            (start, ghost, draw)
                .chain()
                .after(TransformSystems::Propagate),
        );
    }
}

/// The diagnosis overlay in play: the solver's points and bones, their
/// targets, the posed capsules, and red marks on points inside a capsule.
/// `KJK_LOOSE_OVERLAY` draws it and `KJK_LOOSE_GHOST` fades the body.
#[allow(clippy::too_many_arguments)]
pub(crate) fn debug_overlay(
    mut commands: Commands,
    debug: Res<crate::loose_parts::DebugView>,
    cameras: Query<&GlobalTransform, SceneCamera>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut overlay: Local<Option<Handle<Mesh>>>,
    mut ghosted: Local<Option<Entity>>,
    children: Query<&Children>,
    mut worn: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    mut odyssey: ResMut<Assets<OdysseyMaterial>>,
) {
    if let (Some(alpha), Some(root)) = (
        std::env::var("KJK_LOOSE_GHOST")
            .ok()
            .and_then(|v| v.parse::<f32>().ok()),
        debug.root,
    ) && *ghosted != Some(root)
    {
        let mut any = false;
        for descendant in children.iter_descendants(root) {
            let Ok(mut material) = worn.get_mut(descendant) else {
                continue;
            };
            let Some(mut copy) = odyssey.get(&material.0).map(crate::going_view::blended) else {
                continue;
            };
            copy.params.tint.w *= alpha;
            material.0 = odyssey.add(copy);
            any = true;
        }
        if any {
            *ghosted = Some(root);
        }
    }
    if std::env::var("KJK_LOOSE_OVERLAY").is_err() {
        return;
    }
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let eye = camera.translation();
    let right = camera.right().as_vec3();
    let mut quads = Quads::default();
    if let Some(view) = &debug.view {
        let world = |p: Vec3| game_to_bevy() * view.root.point(p);
        for &(ends, radius, leg, _) in &view.capsules {
            let colour = if leg {
                [0.2, 1.0, 0.3, 1.0]
            } else {
                [0.4, 0.6, 1.0, 1.0]
            };
            capsule(
                &mut quads,
                eye,
                (world(ends[0]), world(ends[1])),
                radius,
                colour,
            );
        }
        for &(a, b) in &view.links {
            quads.line(
                eye,
                world(view.positions[a]),
                world(view.positions[b]),
                0.004,
                [0.7, 0.7, 0.7, 1.0],
            );
        }
        for (i, &p) in view.positions.iter().enumerate() {
            if let Some(parent) = view.parents[i] {
                quads.line(
                    eye,
                    world(view.positions[parent]),
                    world(p),
                    0.012,
                    [1.0, 0.6, 0.1, 1.0],
                );
            }
            let target = view.targets[i];
            quads.line(eye, world(p), world(target), 0.003, [0.2, 1.0, 1.0, 1.0]);
            let t = world(target);
            quads.line(
                eye,
                t - right * 0.012,
                t + right * 0.012,
                0.012,
                [0.2, 1.0, 1.0, 1.0],
            );
            let inside = view.capsules.iter().any(|&(ends, radius, _, _)| {
                p.distance(ragdoll::closest_on_segment(p, ends[0], ends[1])) < radius
            });
            let (size, colour) = if inside {
                (0.045, [1.0, 0.0, 0.0, 1.0])
            } else {
                (0.025, [1.0, 1.0, 0.0, 1.0])
            };
            let w = world(p);
            quads.line(
                eye,
                w - right * size / 2.0,
                w + right * size / 2.0,
                size,
                colour,
            );
        }
    }
    let mesh = quads.mesh();
    match &*overlay {
        Some(handle) => {
            if let Some(mut old) = meshes.get_mut(handle) {
                *old = mesh;
            }
        }
        None => {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                NoFrustumCulling,
                NotShadowCaster,
            ));
            *overlay = Some(handle);
        }
    }
}
