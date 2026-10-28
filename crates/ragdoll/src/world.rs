//! The ragdolls' world: their bodies and joints, the level's faces near
//! them, and the drives that pull them toward their targets.

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::{Mat3, Quat, Vec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::*;

use crate::Frame;
use crate::body::{JointKind, Solid, Template};

/// The physics step (s): 120 a second.
const STEP: f32 = 1.0 / 120.0;
/// At most this many steps per call: a long hitch slows the ragdolls
/// instead of piling up steps.
const MAX_STEPS: u32 = 6;
const GRAVITY: f32 = 9.81;
/// How quickly the drives close a gap (s): the time constant of the pull
/// toward the target, on top of moving as it moves.
const HOLD_TIME: f32 = 0.045;
const TONE_TIME: f32 = 0.05;
/// Ball joints swinging further than this (degrees) hold their twist
/// softly (`wide`).
const WIDE_CONE: f32 = 40.0;
/// How quickly a joint twisted past its limit is turned back (s), and by
/// how much (radians) it may be past before it is: a hard stop chatters
/// against the floor.
const TWIST_TIME: f32 = 0.12;
const TWIST_SLACK: f32 = 0.03;
/// The share of the velocity gap a drive at full strength closes each
/// step: below one half the pull settles without ringing.
const GAIN: f32 = 0.4;
/// The strongest turn a joint's muscle makes at full tone (N·m): anything
/// harder bends the joint.
const MAX_TORQUE: f32 = 450.0;
/// The strongest pull of the hold at full strength (m/s² of change,
/// times a body's mass).
const MAX_HOLD: f32 = 120.0;
/// The fastest a body is thrown (m/s): across, and up or down.
const MAX_THROW: (f32, f32) = (5.0, 3.5);
/// The level's friction and bounce (a body's own come from its material).
const FRICTION: f32 = 0.85;
const BOUNCE: f32 = 0.05;
/// Moving bodies lose speed to the air and their own floppiness.
const LINEAR_DAMPING: f32 = 0.05;
const ANGULAR_DAMPING: f32 = 0.6;
/// A level face's `user_data`: no ragdoll's.
const LEVEL: u128 = u128::MAX;
/// A ragdoll let go (no hold) none of whose bodies moves farther than this (metres,
/// radians) in `SETTLE` seconds is put to sleep: it has come to rest. By
/// place, not speed: a joint at its limit and the floor push against each
/// other at a standstill.
const STILL: (f32, f32) = (0.01, 0.05);
const SETTLE: f32 = 0.4;
/// A ragdoll let go and slower than this (m/s, every body) is all but down: its
/// damping rises to `RESTING_DAMPING` so it stops chattering on the floor.
const SLOW: f32 = 0.25;
const RESTING_DAMPING: (f32, f32) = (5.0, 12.0);
/// A ragdoll let go that has been that slow this long (s) is put to sleep
/// however it creeps: a limb whose joint is held at its limit against the
/// floor can roll on for ever, a little too fast to count as still.
const SLOW_FOR: f32 = 1.5;

/// How strongly a ragdoll follows its targets: `hold` pulls each body to
/// its target in the world, `tone` turns each toward its target about its
/// parent (0 to 1 each).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Muscles {
    pub hold: f32,
    pub tone: f32,
}

impl Muscles {
    pub const LIMP: Muscles = Muscles {
        hold: 0.0,
        tone: 0.0,
    };
}

/// A body's mass (kg) and its inertia about its centre (body frame): a
/// box's, or a capsule's as a cylinder of its whole length.
#[derive(Debug, Clone, Copy)]
struct Mass {
    mass: f32,
    inertia: Mat3,
}

impl Mass {
    fn of(mass: f32, solid: &Solid) -> Mass {
        let inertia = match *solid {
            Solid::Capsule {
                ends: [a, b],
                radius,
            } => {
                let length = a.distance(b) + 2.0 * radius;
                let across = mass * (length * length / 12.0 + radius * radius / 4.0);
                let along = mass * radius * radius / 2.0;
                let axis = (b - a).try_normalize().unwrap_or(Vec3::Z);
                let outer = Mat3::from_cols(axis * axis.x, axis * axis.y, axis * axis.z);
                Mat3::IDENTITY * across + outer * (along - across)
            }
            Solid::Cuboid { turn, half, .. } => {
                let size = half * 2.0;
                let squared = size * size;
                let own = Mat3::from_diagonal(
                    Vec3::new(
                        squared.y + squared.z,
                        squared.x + squared.z,
                        squared.x + squared.y,
                    ) * (mass / 12.0),
                );
                let turn = Mat3::from_quat(turn);
                turn * own * turn.transpose()
            }
        };
        Mass { mass, inertia }
    }

    /// The impulse that changes its spin by `spin` (world) turned so.
    fn turning(&self, rotation: Quat, spin: Vec3) -> Vec3 {
        rotation * (self.inertia * (rotation.inverse() * spin))
    }

    /// Its inertia about `axis` (world, unit) turned so.
    fn about(&self, rotation: Quat, axis: Vec3) -> f32 {
        let local = rotation.inverse() * axis;
        local.dot(self.inertia * local).max(1e-5)
    }
}

/// The inertia two bodies share about `axis` turned against each other.
fn shared(a: (&Mass, Quat), b: (&Mass, Quat), axis: Vec3) -> f32 {
    let (a, b) = (a.0.about(a.1, axis), b.0.about(b.1, axis));
    a * b / (a + b)
}

/// A ragdoll in a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

struct Ragdoll {
    template: Arc<Template>,
    bodies: Vec<RigidBodyHandle>,
    masses: Vec<Mass>,
    /// Each body's target (world) and the target's linear and angular
    /// velocity, and when they were set (s).
    targets: Vec<Frame>,
    moving: Vec<(Vec3, Vec3)>,
    targets_at: Option<f64>,
    /// The velocity it was spawned with, and whether it has since been set
    /// moving as its animation moves.
    spawned: Vec3,
    launched: bool,
    muscles: Muscles,
    /// Since when it has lain still: how long (s), and where it was then.
    still: Option<(f32, Vec<Frame>)>,
    /// How long it has been all but down (s).
    slow_for: f32,
    /// Each part's joint to its parent, while it holds.
    joints: Vec<Option<ImpulseJointHandle>>,
    /// The parts cut off the body (bit per part): limp, and touching what
    /// they were cut from.
    cut_off: u64,
}

impl Ragdoll {
    fn cut(&self, part: usize) -> bool {
        self.cut_off & 1 << part != 0
    }
}

/// Ragdolls and the level they fall in.
pub struct World {
    physics: PhysicsWorld,
    ragdolls: Vec<Option<Ragdoll>>,
    /// The level's faces taken in so far, by their key.
    faces: BTreeSet<u32>,
    /// Time not stepped yet (s).
    behind: f32,
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

/// Which pairs collide: a ragdoll's own parts only as its template says.
struct Touching<'a>(&'a [Option<Ragdoll>]);

impl PhysicsHooks for Touching<'_> {
    fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
        let data = |handle| context.colliders.get(handle).map_or(LEVEL, |c| c.user_data);
        let (a, b) = (data(context.collider1), data(context.collider2));
        if a == LEVEL || b == LEVEL || a >> 8 != b >> 8 {
            return Some(SolverFlags::COMPUTE_RIGID_IMPULSES);
        }
        let ragdoll = self.0.get((a >> 8) as usize)?.as_ref()?;
        let (a, b) = ((a & 0xff) as usize, (b & 0xff) as usize);
        (ragdoll.cut(a) != ragdoll.cut(b) || ragdoll.template.touches(a, b))
            .then_some(SolverFlags::COMPUTE_RIGID_IMPULSES)
    }
}

fn vector(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

fn vec3(v: Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rotation(q: Quat) -> Rotation {
    Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

fn quat(q: Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

fn pose(frame: Frame) -> Pose {
    Pose::from_parts(vector(frame.translation), rotation(frame.rotation))
}

fn frame(pose: &Pose) -> Frame {
    Frame::new(vec3(pose.translation), quat(pose.rotation))
}

/// The turn from `from` to `to` as a scaled axis (radians), the shorter
/// way.
fn turn(from: Quat, to: Quat) -> Vec3 {
    let mut delta = to * from.inverse();
    if delta.w < 0.0 {
        delta = -delta;
    }
    let (axis, angle) = delta.to_axis_angle();
    if angle.is_finite() {
        axis * angle
    } else {
        Vec3::ZERO
    }
}

impl World {
    pub fn new() -> World {
        let mut physics = PhysicsWorld::new();
        physics.gravity = Vector::new(0.0, 0.0, -GRAVITY);
        physics.integration_parameters.dt = STEP;
        physics.integration_parameters.num_solver_iterations = 8;
        World {
            physics,
            ragdolls: Vec::new(),
            faces: BTreeSet::new(),
            behind: 0.0,
        }
    }

    /// Takes in the level's faces (world space), each with a key (its
    /// brush): faces under a key taken in by an earlier call are skipped.
    /// One fixed mesh of them.
    pub fn add_faces(&mut self, faces: impl IntoIterator<Item = (u32, [Vec3; 3])>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut added = BTreeSet::new();
        for (key, corners) in faces {
            if self.faces.contains(&key) {
                continue;
            }
            added.insert(key);
            let first = vertices.len() as u32;
            vertices.extend(corners.map(vector));
            indices.push([first, first + 1, first + 2]);
        }
        self.faces.extend(added);
        if indices.is_empty() {
            return;
        }
        let flags = TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED;
        if let Ok(mesh) = ColliderBuilder::trimesh_with_flags(vertices, indices, flags) {
            let mesh = mesh
                .friction(FRICTION)
                .restitution(BOUNCE)
                .user_data(LEVEL)
                .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS);
            self.physics.insert_collider(mesh, None);
        }
    }

    /// A ragdoll of `template`, its bodies where `places` has them (world,
    /// by part), all moving at `velocity`.
    pub fn spawn(&mut self, template: Arc<Template>, places: &[Frame], velocity: Vec3) -> Id {
        let (friction, bounce) = template.material.surface();
        let slot = self
            .ragdolls
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.ragdolls.len());
        let mut bodies: Vec<RigidBodyHandle> = Vec::with_capacity(template.parts.len());
        let mut masses = Vec::with_capacity(template.parts.len());
        let mut joints = Vec::with_capacity(template.parts.len());
        for (index, (part, place)) in template.parts.iter().zip(places).enumerate() {
            let body = RigidBodyBuilder::dynamic()
                .pose(pose(*place))
                .linvel(vector(thrown(velocity)))
                .linear_damping(LINEAR_DAMPING)
                .angular_damping(ANGULAR_DAMPING)
                .ccd_enabled(true);
            let part_mass = part.mass * template.mass;
            let shape = match part.solid {
                Solid::Capsule {
                    ends: [a, b],
                    radius,
                } => ColliderBuilder::capsule_from_endpoints(vector(a), vector(b), radius),
                Solid::Cuboid { centre, turn, half } => {
                    ColliderBuilder::cuboid(half.x, half.y, half.z)
                        .position(pose(Frame::new(centre, turn)))
                }
            };
            let collider = shape
                .mass(part_mass)
                .friction(friction)
                .restitution(bounce)
                .user_data((slot as u128) << 8 | index as u128)
                .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS);
            let (handle, _) = self.physics.insert(body, collider);
            masses.push(Mass::of(part_mass, &part.solid));
            if let (Some(parent), Some(joint)) = (part.parent, part.joint) {
                let data = match joint.kind {
                    JointKind::Ball { cone, twist } => {
                        let ball = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                            .coupled_axes(JointAxesMask::ANG_Y | JointAxesMask::ANG_Z)
                            .limits(JointAxis::AngY, [0.0, cone])
                            .limits(JointAxis::AngZ, [0.0, cone]);
                        if wide(cone) {
                            ball
                        } else {
                            ball.limits(JointAxis::AngX, [-twist, twist])
                        }
                    }
                    JointKind::Hinge { max } => {
                        GenericJointBuilder::new(JointAxesMask::LOCKED_REVOLUTE_AXES)
                            .limits(JointAxis::AngX, [0.0, max])
                    }
                };
                let data = data
                    .local_frame1(pose(joint.on_parent))
                    .local_frame2(pose(joint.on_child))
                    .contacts_enabled(false)
                    .build();
                joints.push(Some(self.physics.insert_impulse_joint(
                    bodies[parent],
                    handle,
                    data,
                )));
            } else {
                joints.push(None);
            }
            bodies.push(handle);
        }
        let ragdoll = Ragdoll {
            targets: places.to_vec(),
            moving: vec![(thrown(velocity), Vec3::ZERO); places.len()],
            targets_at: None,
            spawned: thrown(velocity),
            launched: false,
            template,
            bodies,
            masses,
            muscles: Muscles::LIMP,
            still: None,
            slow_for: 0.0,
            joints,
            cut_off: 0,
        };
        if slot == self.ragdolls.len() {
            self.ragdolls.push(Some(ragdoll));
        } else {
            self.ragdolls[slot] = Some(ragdoll);
        }
        Id(slot as u32)
    }

    /// Where each of the ragdoll's bodies should be at `seconds` (world, by
    /// part), and how strongly it follows: their velocities are taken from
    /// the last targets set.
    pub fn drive(&mut self, id: Id, targets: &[Frame], muscles: Muscles, seconds: f64) {
        let Some(ragdoll) = self
            .ragdolls
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
        else {
            return;
        };
        if let Some(was) = ragdoll.targets_at {
            let dt = (seconds - was) as f32;
            if dt > 1e-4 {
                for ((moving, old), new) in
                    ragdoll.moving.iter_mut().zip(&ragdoll.targets).zip(targets)
                {
                    *moving = (
                        (new.translation - old.translation) / dt,
                        turn(old.rotation, new.rotation) / dt,
                    );
                }
            }
        }
        ragdoll.targets.clear();
        ragdoll.targets.extend_from_slice(targets);
        let first_velocities = ragdoll.targets_at.is_some() && !ragdoll.launched;
        ragdoll.targets_at = Some(seconds);
        if first_velocities {
            ragdoll.launched = true;
            launch(&mut self.physics.bodies, ragdoll);
        }
        ragdoll.muscles = muscles;
        // Only the hold carries a body on: tone alone (a droid's locked
        // servos) leaves one at rest asleep.
        if muscles.hold > 0.0 {
            for &handle in &ragdoll.bodies {
                self.physics.wake_up(handle, true);
            }
        }
    }

    /// A blow: `impulse` (N·s, world) at `point` on part `part`.
    pub fn strike(&mut self, id: Id, part: usize, impulse: Vec3, point: Vec3) {
        let Some(ragdoll) = self.ragdolls.get(id.0 as usize).and_then(Option::as_ref) else {
            return;
        };
        if let Some(body) = ragdoll
            .bodies
            .get(part)
            .and_then(|&handle| self.physics.bodies.get_mut(handle))
        {
            body.apply_impulse_at_point(vector(impulse), vector(point), true);
        }
    }

    /// Cuts part `part` (and all that hangs on it) off the body: its joint
    /// goes, it goes limp, it touches what it was cut from, and it is sent
    /// off by `impulse` (N·s, world) at `point` with `spin` (N·m·s), as far
    /// on as that takes it in `ahead` (s: a cut seen late). False for the
    /// root, or a part already cut off or hanging from one.
    pub fn sever(
        &mut self,
        id: Id,
        part: usize,
        (impulse, point, spin): (Vec3, Vec3, Vec3),
        ahead: f32,
    ) -> bool {
        let Some(ragdoll) = self
            .ragdolls
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
        else {
            return false;
        };
        // A part already cut, or hanging from one, keeps its joint.
        if ragdoll.cut(part) {
            return false;
        }
        let Some(joint) = ragdoll.joints.get_mut(part).and_then(Option::take) else {
            return false;
        };
        self.physics.remove_impulse_joint(joint);
        // The parts below: those whose chain of parents reaches it.
        let parts = &ragdoll.template.parts;
        let mut loose = 0u64;
        for index in 0..parts.len() {
            let mut at = Some(index);
            while let Some(p) = at {
                if p == part {
                    loose |= 1 << index;
                    break;
                }
                at = parts[p].parent;
            }
        }
        ragdoll.cut_off |= loose;
        let Some(body) = self.physics.bodies.get_mut(ragdoll.bodies[part]) else {
            return true;
        };
        body.apply_impulse_at_point(vector(impulse), vector(point), true);
        body.apply_torque_impulse(vector(spin), true);
        // The limb and all that hangs on it moved on together.
        let shift = vector(vec3(body.linvel()) * ahead);
        for (index, &handle) in ragdoll.bodies.iter().enumerate() {
            if loose & 1 << index != 0
                && let Some(body) = self.physics.bodies.get_mut(handle)
            {
                let to = body.translation() + shift;
                body.set_translation(to, true);
            }
        }
        true
    }

    /// Takes the ragdoll out of the world, its bodies and joints with it:
    /// its slot is free for the next.
    pub fn remove(&mut self, id: Id) {
        let Some(ragdoll) = self.ragdolls.get_mut(id.0 as usize).and_then(Option::take) else {
            return;
        };
        for handle in ragdoll.bodies {
            self.physics.remove_body(handle);
        }
    }

    /// Whether part `part` of the ragdoll is cut off its body.
    pub fn cut_off(&self, id: Id, part: usize) -> bool {
        self.ragdolls
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .is_some_and(|ragdoll| ragdoll.cut(part))
    }

    /// Each body of the ragdoll (world, by part).
    pub fn places(&self, id: Id) -> Option<Vec<Frame>> {
        let ragdoll = self.ragdolls.get(id.0 as usize)?.as_ref()?;
        ragdoll
            .bodies
            .iter()
            .map(|&handle| Some(frame(self.physics.bodies.get(handle)?.position())))
            .collect()
    }

    /// Whether every body of the ragdoll has come to rest (and sleeps).
    pub fn resting(&self, id: Id) -> bool {
        self.ragdolls
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .is_some_and(|ragdoll| asleep(&self.physics.bodies, ragdoll))
    }

    /// How many ragdolls there are, and how many bodies move.
    pub fn counts(&self) -> (usize, usize) {
        let ragdolls = self.ragdolls.iter().flatten().count();
        let moving = self
            .physics
            .bodies
            .iter()
            .filter(|(_, body)| body.is_dynamic() && !body.is_sleeping())
            .count();
        (ragdolls, moving)
    }

    /// Runs the world on by `seconds`, in fixed steps.
    pub fn step(&mut self, seconds: f32) {
        self.behind = (self.behind + seconds.max(0.0)).min(STEP * MAX_STEPS as f32);
        while self.behind >= STEP {
            self.behind -= STEP;
            for ragdoll in self.ragdolls.iter().flatten() {
                if asleep(&self.physics.bodies, ragdoll) {
                    continue;
                }
                drive(&mut self.physics.bodies, ragdoll);
                rub(&mut self.physics.bodies, ragdoll);
                for (child, parent, pull) in limit_twists(&self.physics.bodies, ragdoll) {
                    turn_apart(&mut self.physics.bodies, ragdoll, (child, parent), pull);
                }
            }
            let hooks = Touching(&self.ragdolls);
            self.physics.step_with_events(&hooks, &());
            for ragdoll in self.ragdolls.iter_mut().flatten() {
                settle(&mut self.physics.bodies, ragdoll);
            }
        }
    }
}

/// Puts a ragdoll let go to sleep once it has lain still for `SETTLE`, or
/// been all but down for `SLOW_FOR`; one all but down is damped hard
/// meanwhile.
fn settle(bodies: &mut RigidBodySet, ragdoll: &mut Ragdoll) {
    if ragdoll.muscles.hold > 0.0 || asleep(bodies, ragdoll) {
        ragdoll.still = None;
        ragdoll.slow_for = 0.0;
        return;
    }
    let slow = ragdoll
        .bodies
        .iter()
        .filter_map(|&handle| bodies.get(handle))
        .all(|body| body.linvel().length() < SLOW);
    ragdoll.slow_for = if slow { ragdoll.slow_for + STEP } else { 0.0 };
    let (linear, angular) = if slow {
        RESTING_DAMPING
    } else {
        (LINEAR_DAMPING, ANGULAR_DAMPING)
    };
    let mut places = Vec::with_capacity(ragdoll.bodies.len());
    for &handle in &ragdoll.bodies {
        if let Some(body) = bodies.get_mut(handle) {
            body.set_linear_damping(linear);
            body.set_angular_damping(angular);
            places.push(frame(body.position()));
        }
    }
    let still = |then: &[Frame]| {
        then.iter().zip(&places).all(|(a, b)| {
            a.translation.distance(b.translation) < STILL.0
                && a.rotation.angle_between(b.rotation) < STILL.1
        })
    };
    let settled = match &mut ragdoll.still {
        Some((seconds, then)) if still(then) => {
            *seconds += STEP;
            *seconds >= SETTLE
        }
        _ => {
            ragdoll.still = Some((0.0, places));
            false
        }
    };
    if settled || ragdoll.slow_for >= SLOW_FOR {
        for &handle in &ragdoll.bodies {
            if let Some(body) = bodies.get_mut(handle) {
                body.sleep();
            }
        }
        ragdoll.still = None;
        ragdoll.slow_for = 0.0;
    }
}

/// Whether every body of the ragdoll sleeps.
fn asleep(bodies: &RigidBodySet, ragdoll: &Ragdoll) -> bool {
    ragdoll
        .bodies
        .iter()
        .all(|&handle| bodies.get(handle).is_none_or(RigidBody::is_sleeping))
}

/// A turn `pull` (N·m·s) on part `child`, and the same the other way on
/// `parent`: what a joint's own forces do.
fn turn_apart(
    bodies: &mut RigidBodySet,
    ragdoll: &Ragdoll,
    (child, parent): (usize, usize),
    pull: Vec3,
) {
    for (part, pull) in [(child, pull), (parent, -pull)] {
        if let Some(body) = bodies.get_mut(ragdoll.bodies[part]) {
            body.apply_torque_impulse(vector(pull), true);
        }
    }
}

/// Each joint's friction, once the hold has let go: it takes a share of
/// the joint's relative spin and holds it fast below a spin, so a limp
/// arm on the floor does not roll on for ever under its forearm's weight.
fn rub(bodies: &mut RigidBodySet, ragdoll: &Ragdoll) {
    if ragdoll.muscles.hold > 0.0 {
        return;
    }
    let (share, fast) = ragdoll.template.material.joint_friction();
    for (index, part) in ragdoll.template.parts.iter().enumerate() {
        let Some(parent) = part.parent.filter(|_| ragdoll.joints[index].is_some()) else {
            continue;
        };
        let (Some(child), Some(held)) = (
            bodies.get(ragdoll.bodies[index]),
            bodies.get(ragdoll.bodies[parent]),
        ) else {
            continue;
        };
        let spin = vec3(child.angvel()) - vec3(held.angvel());
        let Some(axis) = spin.try_normalize() else {
            continue;
        };
        // Held fast: all of a slow spin, at once.
        let taken = if spin.length() < fast { 1.0 } else { share };
        let inertia = shared(
            (&ragdoll.masses[index], quat(*child.rotation())),
            (&ragdoll.masses[parent], quat(*held.rotation())),
            axis,
        );
        turn_apart(bodies, ragdoll, (index, parent), -spin * inertia * taken);
    }
}

/// Whether a ball joint swings so far that its twist is held by
/// `limit_twists`: Rapier's twist limit pushes about the parent's axis,
/// which fights the cone once the bone swings far from it; below this a
/// hard limit holds (a droid's stiff joints).
fn wide(cone: f32) -> bool {
    cone > WIDE_CONE.to_radians()
}

/// Turns back each wide ball joint twisted past its limit, about the bone's own
/// axis (the joint frame's X on the child), the parent turned the other
/// way.
fn limit_twists(bodies: &RigidBodySet, ragdoll: &Ragdoll) -> Vec<(usize, usize, Vec3)> {
    let mut pulls = Vec::new();
    for (index, part) in ragdoll.template.parts.iter().enumerate() {
        let (Some(parent), Some(joint), Some(_)) = (part.parent, part.joint, ragdoll.joints[index])
        else {
            continue;
        };
        let JointKind::Ball { twist: limit, cone } = joint.kind else {
            continue;
        };
        if !wide(cone) {
            continue;
        }
        let (Some(child), Some(held)) = (
            bodies.get(ragdoll.bodies[index]),
            bodies.get(ragdoll.bodies[parent]),
        ) else {
            continue;
        };
        let on_child = quat(*child.rotation()) * joint.on_child.rotation;
        let on_parent = quat(*held.rotation()) * joint.on_parent.rotation;
        let relative = on_parent.inverse() * on_child;
        let relative = if relative.w < 0.0 {
            -relative
        } else {
            relative
        };
        let twist = 2.0 * relative.x.atan2(relative.w);
        let excess = twist - twist.clamp(-limit, limit);
        if excess.abs() <= TWIST_SLACK {
            continue;
        }
        let axis = on_child * Vec3::X;
        let spin = (vec3(child.angvel()) - vec3(held.angvel())).dot(axis);
        let wanted = -excess / TWIST_TIME;
        // Only ever back toward the limit.
        let change = if excess > 0.0 {
            (wanted - spin).min(0.0)
        } else {
            (wanted - spin).max(0.0)
        };
        let inertia = shared(
            (&ragdoll.masses[index], quat(*child.rotation())),
            (&ragdoll.masses[parent], quat(*held.rotation())),
            axis,
        );
        pulls.push((index, parent, axis * change * inertia * GAIN));
    }
    pulls
}

/// `velocity` as fast as a body is ever thrown: JA flings its dead at the
/// speed of its knockback, far past what a blow does to a person.
fn thrown(velocity: Vec3) -> Vec3 {
    let across = velocity.with_z(0.0).clamp_length_max(MAX_THROW.0);
    across.with_z(velocity.z.clamp(-MAX_THROW.1, MAX_THROW.1))
}

/// Sets the whole body moving as its animation moves it (its centre of
/// mass's velocity), once, in place of the velocity it was spawned with:
/// what struck it since (the killing blow) stays. From then on only
/// momentum, gravity and what it hits carry it, the drives being internal.
fn launch(bodies: &mut RigidBodySet, ragdoll: &Ragdoll) {
    let (mut mass, mut wanted) = (0.0, Vec3::ZERO);
    for ((&handle, body), (target, _)) in ragdoll
        .bodies
        .iter()
        .zip(&ragdoll.masses)
        .zip(&ragdoll.moving)
    {
        if bodies.get(handle).is_some() {
            mass += body.mass;
            wanted += *target * body.mass;
        }
    }
    if mass <= 0.0 {
        return;
    }
    let change = thrown(wanted / mass) - ragdoll.spawned;
    for &handle in &ragdoll.bodies {
        if let Some(rigid) = bodies.get_mut(handle) {
            let linvel = vec3(rigid.linvel()) + change;
            rigid.set_linvel(vector(linvel), true);
        }
    }
}

/// One step of a ragdoll's drives, as impulses: the hold on each body
/// toward its place in the animation, less their sum (so it shapes the
/// body but never carries it: a puppet's strings would hold it up in the
/// air), and the tone between each body and its parent (equal and
/// opposite, as a muscle pulls).
fn drive(bodies: &mut RigidBodySet, ragdoll: &Ragdoll) {
    let Muscles { hold, tone } = ragdoll.muscles;
    if hold <= 0.0 && tone <= 0.0 {
        return;
    }
    let state: Vec<Option<(Frame, Vec3, Vec3)>> = ragdoll
        .bodies
        .iter()
        .map(|&handle| {
            let body = bodies.get(handle)?;
            Some((
                frame(body.position()),
                vec3(body.linvel()),
                vec3(body.angvel()),
            ))
        })
        .collect();
    let mut pushes = vec![Vec3::ZERO; state.len()];
    for (index, part) in ragdoll.template.parts.iter().enumerate() {
        let Some((place, linvel, angvel)) = state[index].filter(|_| !ragdoll.cut(index)) else {
            continue;
        };
        let target = ragdoll.targets[index];
        let (target_linvel, target_angvel) = ragdoll.moving[index];
        let body = &ragdoll.masses[index];
        let mut push = Vec3::ZERO;
        let mut twist = Vec3::ZERO;
        if hold > 0.0 {
            let wanted = target_linvel + (target.translation - place.translation) / HOLD_TIME;
            push = ((wanted - linvel) * body.mass * GAIN * hold)
                .clamp_length_max(MAX_HOLD * body.mass * STEP * hold);
            let wanted = target_angvel + turn(place.rotation, target.rotation) / HOLD_TIME;
            twist = body.turning(place.rotation, (wanted - angvel) * GAIN * hold);
        }
        // The muscle about the parent's joint.
        if let (true, Some(parent)) = (tone > 0.0, part.parent)
            && let Some((parent_place, _, parent_angvel)) = state[parent]
        {
            let parent_target = ragdoll.targets[parent];
            let (_, parent_target_angvel) = ragdoll.moving[parent];
            let wanted_relative = parent_target.rotation.inverse() * target.rotation;
            let wanted = parent_place.rotation * wanted_relative;
            let error = turn(place.rotation, wanted);
            let wanted_spin = (target_angvel - parent_target_angvel) + error / TONE_TIME;
            let change = wanted_spin - (angvel - parent_angvel);
            let inertia = change.try_normalize().map_or(0.0, |axis| {
                shared(
                    (body, place.rotation),
                    (&ragdoll.masses[parent], parent_place.rotation),
                    axis,
                )
            });
            let muscle =
                (change * inertia * GAIN * tone).clamp_length_max(MAX_TORQUE * STEP * tone);
            turn_apart(bodies, ragdoll, (index, parent), muscle);
        }
        pushes[index] = push;
        if let Some(body) = bodies.get_mut(ragdoll.bodies[index]) {
            body.apply_torque_impulse(vector(twist), true);
        }
    }
    // Netted over the body still whole: what was cut off goes its own way.
    let whole = |index: &usize| !ragdoll.cut(*index);
    let mass: f32 = (0..pushes.len())
        .filter(whole)
        .map(|i| ragdoll.masses[i].mass)
        .sum();
    let net = pushes.iter().sum::<Vec3>() / mass.max(1e-3);
    for index in (0..pushes.len()).filter(whole) {
        if let Some(rigid) = bodies.get_mut(ragdoll.bodies[index]) {
            let push = pushes[index] - net * ragdoll.masses[index].mass;
            rigid.apply_impulse(vector(push), true);
        }
    }
}
