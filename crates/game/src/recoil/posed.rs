//! The firing motion measured on a posed KOTOR body. JA's ready pose for a
//! rifle (aimed and from the hip) and for a pair of pistols is retargeted
//! to Carth's body (`p_carthbb`) with K1's guns held up
//! (`asset_ja::rig::Aim`) and pushed by a shot (`push`). The body's turns
//! (`turns`) are added after the arms are posed, as `cast::recoil` adds
//! them. Two checks follow a JA figure over time instead of one held pose:
//! [Aim] pressed and let go of around a rifle shot, and a rifle fired once a
//! second while it breathes. Carth's head (`p_carthh`) shows how near a
//! raised rifle comes to the head. These checks need JA and
//! KOTOR installed.

use asset_ja::rig::{Aim, OffHand, Posed, Side};
use bevy::prelude::*;
use sim::probe::check;

use super::*;
use crate::aim_pose_probes::{AIM_FROM, Ja, NEVER, run, standing};
use crate::cast::BodyRig;
use crate::weapons::GunShape;

/// How much further (metres) from its grip on the rifle the support hand
/// may come during a shot.
const SUPPORT_MOST: f32 = 0.01;
/// The most a rifle's muzzle may move sideways during a shot, as a share of
/// how far the shot moves it back along the barrel.
const SIDEWAYS_MOST: f32 = 0.5;
/// The least (metres) between a raised rifle and the head.
const HEAD_CLEAR: f32 = 0.01;
/// The most (metres) between a raised rifle and the head resting its cheek
/// on the stock, before a shot.
const CHEEK_NEAR: f32 = 0.03;
/// The most (metres) the right eye may stand to the side of a raised
/// rifle's barrel.
const EYE_ASIDE: f32 = 0.04;
/// How far (metres) the left gun of a pair may move while the right one
/// fires.
const STILL_MOST: f32 = 0.003;
/// Where the support hand holds a rifle, as a share of the way to its
/// muzzle (`weapons::handling`).
const RIFLE_SUPPORT: f32 = 0.5;
/// How far ahead of the muzzle the gun aims (metres).
const AHEAD: f32 = 10.0;
/// One frame at 125 frames a second (ms).
const STEP_MS: f64 = 8.0;
/// The moments sampled around a shot at 0 (ms).
const SPAN_MS: (f64, f64) = (-40.0, 600.0);

/// The body, its guns and JA's skeleton. Also Carth's head (`p_carthh`) as
/// the points of its mesh under the body's `headhook`, with the node it
/// turns by (`hturn_g`) and its right eye (`eyera`) in its own space, and
/// the rifle as the points of its mesh under the hand's weapon node.
struct Scene {
    ja: Ja,
    body: BodyRig,
    rifle: GunShape,
    pistol: GunShape,
    head: Vec<Vec3>,
    head_turns_at: Vec3,
    right_eye: Vec3,
    rifle_points: Vec<Vec3>,
    skin: Skin,
}

/// Carth's body mesh moved by its bones as the renderer moves it: each
/// vertex by up to four nodes, by their weights. Also the mesh's triangles.
struct Skin {
    vertices: Vec<Vertex>,
    triangles: Vec<[usize; 3]>,
    /// Each node's place in the model's space at rest.
    rest: Vec<(Vec3, Quat)>,
}

/// A vertex of the body's mesh at rest (the model's space): the node that
/// moves it most, and every node that moves it with its weight.
struct Vertex {
    node: usize,
    point: Vec3,
    normal: Vec3,
    bones: Vec<(usize, f32)>,
}

impl Skin {
    fn new(ir: &asset_odyssey::model::ModelIr, rest: Vec<(Vec3, Quat)>) -> Skin {
        let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
        for mesh in &ir.meshes {
            let Some(skin) = &mesh.skin else {
                continue;
            };
            let place = crate::scene::to_mat4(&ir.nodes[mesh.node].world);
            let mut kept = vec![None; mesh.positions.len()];
            let weighted = skin.vertex_joints.iter().zip(&skin.vertex_weights);
            for (index, (joints, weights)) in weighted.enumerate() {
                let bones: Vec<(usize, f32)> = (0..4)
                    .filter(|&slot| weights[slot] > 0.0)
                    .filter_map(|slot| {
                        let node = *skin.joints.get(usize::from(joints[slot]))?;
                        Some((node, weights[slot]))
                    })
                    .collect();
                let most = bones.iter().max_by(|a, b| a.1.total_cmp(&b.1));
                let (Some(&(node, _)), Some(position), Some(normal)) =
                    (most, mesh.positions.get(index), mesh.normals.get(index))
                else {
                    continue;
                };
                kept[index] = Some(vertices.len());
                vertices.push(Vertex {
                    node,
                    point: place.transform_point3(Vec3::from_array(*position)),
                    normal: place.transform_vector3(Vec3::from_array(*normal)),
                    bones,
                });
            }
            for corners in mesh.indices.as_chunks::<3>().0 {
                let found = corners.map(|i| kept.get(i as usize).copied().flatten());
                if let [Some(a), Some(b), Some(c)] = found {
                    triangles.push([a, b, c]);
                }
            }
        }
        Skin {
            vertices,
            triangles,
            rest,
        }
    }

    /// Each vertex's place and normal on the body posed as `globals`.
    fn posed(&self, globals: &[(Vec3, Quat)]) -> Vec<(Vec3, Vec3)> {
        (self.vertices.iter())
            .map(|vertex| {
                let total: f32 = vertex.bones.iter().map(|bone| bone.1).sum();
                let (mut point, mut normal) = (Vec3::ZERO, Vec3::ZERO);
                for &(node, weight) in &vertex.bones {
                    let ((was, was_turn), (now, turn)) = (self.rest[node], globals[node]);
                    let moved = turn * was_turn.inverse();
                    point += (now + moved * (vertex.point - was)) * weight;
                    normal += moved * vertex.normal * weight;
                }
                (point / total.max(1e-6), normal.normalize_or_zero())
            })
            .collect()
    }
}

/// Where the node `name` of the KOTOR model `model` stands in its own space.
fn node_at(discovery: &vfs::Discovery, (model, name): (&str, &str)) -> Result<Vec3, String> {
    let models = crate::viewer::load_model(discovery, "k1", model, None)?.models;
    let ir = &models.first().ok_or(format!("{model}: no model"))?.ir;
    let node = (ir.nodes.iter())
        .find(|node| node.name.eq_ignore_ascii_case(name))
        .ok_or(format!("{model} has no {name}"))?;
    Ok(crate::scene::to_mat4(&node.world).w_axis.truncate())
}

/// Every point of the meshes of the KOTOR model `name`, in its own space.
fn points(discovery: &vfs::Discovery, name: &str) -> Result<Vec<Vec3>, String> {
    let models = crate::viewer::load_model(discovery, "k1", name, None)?.models;
    let ir = &models.first().ok_or(format!("{name}: no model"))?.ir;
    Ok(ir
        .meshes
        .iter()
        .flat_map(|mesh| {
            let place = crate::scene::to_mat4(&ir.nodes[mesh.node].world);
            (mesh.positions.iter()).map(move |p| place.transform_point3(Vec3::from_array(*p)))
        })
        .collect())
}

/// A K1 model as a gun's shape (`weapons::arm`).
fn gun_shape(
    discovery: &vfs::Discovery,
    name: &str,
    sabers: &crate::weapons::Sabers,
) -> Result<GunShape, String> {
    let mut models = crate::viewer::load_model(discovery, "k1", name, None)?.models;
    let [held, _] = crate::weapons::arm(
        &mut models,
        &mut Vec::new(),
        [Some(0), None],
        sabers,
        &mut Vec::new(),
    );
    held.and_then(|held| held.gun)
        .ok_or(format!("{name}: no gun shape"))
}

impl Scene {
    fn load() -> Result<Scene, String> {
        let ja = Ja::load()?;
        let discovery = crate::discover();
        let body = crate::viewer::load_model(&discovery, "k1", "p_carthbb", None)?;
        let body = BodyRig::new(&body.models[0].ir, &ja.rig, Vec::new());
        let rifle = gun_shape(&discovery, "w_blstrrfl_001", &ja.rig.sabers)?;
        let pistol = gun_shape(&discovery, "w_blstrpstl_001", &ja.rig.sabers)?;
        let head = points(&discovery, "p_carthh")?;
        let rifle_points = points(&discovery, "w_blstrrfl_001")?;
        let rest = rest_places(&body.rest);
        let skin = Skin::new(body.model(), rest);
        Ok(Scene {
            ja,
            body,
            rifle,
            pistol,
            head,
            head_turns_at: node_at(&discovery, ("p_carthh", "hturn_g"))?,
            right_eye: node_at(&discovery, ("p_carthh", "eyera"))?,
            rifle_points,
            skin,
        })
    }

    /// The gun `wield` holds: the rifle, or the pistol in each hand.
    fn shape(&self, wield: Wield) -> &GunShape {
        match wield {
            Wield::Rifle => &self.rifle,
            _ => &self.pistol,
        }
    }

    /// The index of the body's node `name`.
    fn node(&self, name: &str) -> Result<usize, String> {
        let model = self.body.model();
        (0..model.nodes.len())
            .find(|&node| model.nodes[node].name.eq_ignore_ascii_case(name))
            .ok_or(format!("the body has no {name}"))
    }

    /// JA's pose of a body standing ready with `wield`'s gun, fully aimed
    /// or not.
    fn ja_pose(&self, wield: Wield, aimed: bool) -> Result<Vec<asset_ja::g2::Local>, String> {
        let release = if aimed { NEVER } else { 0 };
        let end = AIM_FROM + 600;
        let poses = run(
            &self.ja,
            (wield, true, 0..=end),
            standing(&self.ja, wield, release),
            |figure, now| figure.pose(now as f64),
        )?;
        poses.into_iter().last().ok_or("no pose".into())
    }

    /// Each node's place in the model's space (position, rotation): the
    /// rest pose with `posed` over it, then each of `after`'s turns in the
    /// model's space, in order (`cast::turn_in_model`).
    fn globals(&self, posed: &Posed, after: &[(usize, Quat)]) -> Vec<(Vec3, Quat)> {
        let rest = &self.body.rest;
        let mut locals: Vec<(Vec3, Quat)> = rest.iter().map(|&(_, at, turn)| (at, turn)).collect();
        for &(node, rotation) in &posed.rotations {
            locals[node].1 = rotation;
        }
        if let Some((root, offset)) = posed.root {
            locals[root].0 = rest[root].1 + self.body.animation_scale * offset;
        }
        let place = |locals: &[(Vec3, Quat)]| {
            let mut globals: Vec<Option<(Vec3, Quat)>> = vec![None; locals.len()];
            fn of(
                node: usize,
                rest: &[(Option<usize>, Vec3, Quat)],
                locals: &[(Vec3, Quat)],
                globals: &mut [Option<(Vec3, Quat)>],
            ) -> (Vec3, Quat) {
                if let Some(done) = globals[node] {
                    return done;
                }
                let (at, turn) = locals[node];
                let done = match rest[node].0 {
                    Some(parent) => {
                        let (p_at, p_turn) = of(parent, rest, locals, globals);
                        (p_at + p_turn * at, p_turn * turn)
                    }
                    None => (at, turn),
                };
                globals[node] = Some(done);
                done
            }
            (0..locals.len())
                .map(|node| of(node, rest, locals, &mut globals))
                .collect::<Vec<_>>()
        };
        for &(node, turn) in after {
            let globals = place(&locals);
            let above = rest[node]
                .0
                .map_or(Quat::IDENTITY, |parent| globals[parent].1);
            locals[node].1 = (above.inverse() * turn * above * locals[node].1).normalize();
        }
        place(&locals)
    }
}

/// A gun held up toward `target`, pushed by `push`.
fn aim(
    side: Side,
    shape: &GunShape,
    (target, raise): (Vec3, f32),
    push: Push,
    support: Option<f32>,
) -> Aim {
    Aim {
        side,
        target,
        muzzle: shape.muzzle,
        barrel: shape.barrel,
        up: shape.up,
        support: support.map(|share| shape.foregrip(share)),
        butt: support.map(|_| (shape.butt, raise)),
        forward: 0.0,
        kick: (push.up_deg.to_radians(), push.back_m),
        shoulder: push.shoulder_deg.to_radians(),
        chest: push.chest_deg.to_radians(),
        weight: 1.0,
        hold: None,
        stow: None,
    }
}

/// One moment of a posed body firing: each node's place.
type Moment = (f64, Vec<(Vec3, Quat)>);

/// A body standing with `wield`'s guns, each a `blaster`, aimed at targets
/// `AHEAD` of where its muzzles stand in the pose it was set up in. It keeps
/// its support hand's ease from one moment to the next.
struct Shooter<'a> {
    scene: &'a Scene,
    wield: Wield,
    blaster: Blaster,
    off: OffHand,
    targets: [Vec3; 2],
    /// The body's right axis and its forward axis (the model's space).
    right: Vec3,
    forward: Vec3,
}

impl<'a> Shooter<'a> {
    fn new(
        scene: &'a Scene,
        (wield, blaster): (Wield, Blaster),
        pose: &[asset_ja::g2::Local],
    ) -> Result<Shooter<'a>, String> {
        let mut off = OffHand::default();
        let retargeter = &scene.body.retargeter;
        let still = retargeter.pose(pose, Vec3::ZERO, (&[], &[]), (&mut off, 0.0), 0.0);
        let still = scene.globals(&still, &[]);
        // A KOTOR body faces +Y, its right +X: the way the player's view
        // looks while it aims.
        let (right, forward) = (Vec3::X, Vec3::Y);
        let weapons = [scene.node("rhand")?, scene.node("lhand")?];
        let muzzle = scene.shape(wield).muzzle;
        let targets = weapons.map(|node| {
            let (at, turn) = still[node];
            at + turn * muzzle + forward * AHEAD
        });
        Ok(Shooter {
            scene,
            wield,
            blaster,
            off,
            targets,
            right,
            forward,
        })
    }

    /// The same, its guns aimed `pitch` (degrees, up) above level.
    fn pitched(mut self, pitch: f32) -> Shooter<'a> {
        let way = Quat::from_axis_angle(self.right, pitch.to_radians()) * self.forward;
        self.targets = self
            .targets
            .map(|target| target - self.forward * AHEAD + way * AHEAD);
        self
    }

    /// The hands that hold a gun, and their sides.
    fn hands(&self) -> &'static [(usize, Side)] {
        match self.wield {
            Wield::Pair => &[(0, Side::Right), (1, Side::Left)],
            _ => &[(0, Side::Right)],
        }
    }

    /// What the firing motion needs of the body at a moment: its right
    /// hand's last shot (`shot`, sim ms) and how far its guns are raised.
    fn firing(&self, shot: Option<f64>, raise: f32) -> Firing {
        Firing {
            wield: Some(self.wield),
            blasters: self.hands().iter().fold([None; 2], |mut guns, &(hand, _)| {
                guns[hand] = Some(self.blaster);
                guns
            }),
            shots: Shots {
                last: [shot, None],
                before: [None; 2],
            },
            streak: Streak::default(),
            raise,
            looked_through: false,
            alive: true,
        }
    }

    /// Each node's place at `now` (sim ms) with JA's `pose` retargeted, the
    /// guns aimed and pushed as `firing` says (`push`), and the body's
    /// turns added after (`turns`). Without `moving` the guns are aimed
    /// but never pushed and the body never turned.
    fn at(
        &mut self,
        pose: &[asset_ja::g2::Local],
        firing: &Firing,
        (now, moving): (f64, bool),
    ) -> Vec<(Vec3, Quat)> {
        let scene = self.scene;
        let shape = scene.shape(self.wield);
        let support = (self.wield == Wield::Rifle).then_some(RIFLE_SUPPORT);
        let aims: Vec<Aim> = (self.hands().iter())
            .map(|&(hand, side)| {
                let push = if moving {
                    push(firing, hand, now)
                } else {
                    Push::default()
                };
                aim(
                    side,
                    shape,
                    (self.targets[hand], firing.raise),
                    push,
                    support,
                )
            })
            .collect();
        let retargeter = &scene.body.retargeter;
        let off = (&mut self.off, now as f32);
        let posed = retargeter.pose(pose, Vec3::ZERO, (&[], &aims), off, 0.0);
        let turned = if moving {
            turns(firing, now)
        } else {
            Vec::new()
        };
        let mut after: Vec<(usize, Quat)> = (turned.iter())
            .filter_map(|turn| {
                let node = scene.node(turn.node).ok()?;
                Some((node, rotation(turn, self.right, Vec3::Z)))
            })
            .collect();
        // The support hand takes the gun again after the body's turns, as
        // `cast::regrip` does.
        let globals = scene.globals(&posed, &after);
        let [aim] = aims.as_slice() else {
            return globals;
        };
        let regrip = retargeter.regrip(&globals, aim);
        if regrip.is_empty() {
            return globals;
        }
        for (node, global) in regrip {
            let now = scene.globals(&posed, &after)[node].1;
            after.push((node, (global * now.inverse()).normalize()));
        }
        scene.globals(&posed, &after)
    }
}

/// A body standing ready in `pose` with `wield`'s guns, each a `blaster`
/// raised as far as `raise` says, whose right hand fires once at 0, posed
/// every `STEP_MS` over `SPAN_MS`. Also its right axis and its forward
/// axis (the model's space).
fn fired(
    scene: &Scene,
    (wield, blaster, raise): (Wield, Blaster, f32),
    pose: &[asset_ja::g2::Local],
) -> Result<(Vec<Moment>, Vec3, Vec3), String> {
    fired_pitched(scene, (wield, blaster, raise), pose, 0.0)
}

/// The same, the guns aimed `pitch` degrees above level.
fn fired_pitched(
    scene: &Scene,
    (wield, blaster, raise): (Wield, Blaster, f32),
    pose: &[asset_ja::g2::Local],
    pitch: f32,
) -> Result<(Vec<Moment>, Vec3, Vec3), String> {
    let mut shooter = Shooter::new(scene, (wield, blaster), pose)?.pitched(pitch);
    let firing = shooter.firing(Some(0.0), raise);
    let mut moments = Vec::new();
    let mut now = SPAN_MS.0;
    while now <= SPAN_MS.1 {
        moments.push((now, shooter.at(pose, &firing, (now, true))));
        now += STEP_MS;
    }
    Ok((moments, shooter.right, shooter.forward))
}

/// The largest distance of `points` from the first of them.
fn drift(points: impl IntoIterator<Item = Vec3>) -> f32 {
    let mut points = points.into_iter();
    let Some(first) = points.next() else {
        return 0.0;
    };
    points
        .map(|point| point.distance(first))
        .fold(0.0, f32::max)
}

/// The moment nearest `ms` (the first moment is before the shot).
fn at(moments: &[Moment], ms: f64) -> &[(Vec3, Quat)] {
    let nearest = moments
        .iter()
        .min_by(|a, b| (a.0 - ms).abs().total_cmp(&(b.0 - ms).abs()));
    nearest.map_or(&[], |(_, globals)| globals.as_slice())
}

/// A rifle shot, aimed and from the hip: the support hand comes no more
/// than `SUPPORT_MOST` further from its grip on the gun than before the
/// shot. At the shoulder's peak the gun-side
/// shoulder goes back by at least 3 mm, and up by no more than half as
/// far. At the push's peak the muzzle goes back along the barrel by more
/// than it goes up.
fn rifle(scene: &Scene) -> Result<String, String> {
    let (gun, hand, shoulder) = (
        scene.node("rhand")?,
        scene.node("lhand_g")?,
        scene.node("rbicep_g")?,
    );
    let peak = f64::from(crate::weapons::handling(Blaster::Rifle).rise_ms);
    let (mut seen, mut failed) = (Vec::new(), false);
    for aimed in [true, false] {
        let pose = scene.ja_pose(Wield::Rifle, aimed)?;
        let raise = if aimed { 1.0 } else { 0.0 };
        let (moments, right, forward) = fired(scene, (Wield::Rifle, Blaster::Rifle, raise), &pose)?;
        // How far the support hand is from where it holds the gun. Where
        // the pose holds the gun out of the arm's reach, the hand reaches
        // toward that place, so only a gap that grows counts.
        let grip = scene.rifle.foregrip(RIFLE_SUPPORT);
        let gap = |globals: &[(Vec3, Quat)]| {
            let (at, turn) = globals[gun];
            globals[hand].0.distance(at + turn * grip)
        };
        let rest = gap(&moments[0].1);
        let support = moments
            .iter()
            .map(|(_, globals)| gap(globals) - rest)
            .fold(0.0, f32::max);
        let before = &moments[0].1;
        let moved = at(&moments, RISE_MS)[shoulder].0 - before[shoulder].0;
        let (back, up) = (-moved.dot(forward), moved.z);
        let muzzle = |globals: &[(Vec3, Quat)]| {
            let (at, turn) = globals[gun];
            (at + turn * scene.rifle.muzzle, turn * scene.rifle.barrel)
        };
        let ((was, barrel), (now, _)) = (muzzle(before), muzzle(at(&moments, peak)));
        let along = -(now - was).dot(barrel);
        let lift = (now - was).z;
        let sideways = moments
            .iter()
            .map(|(_, globals)| (muzzle(globals).0 - was).dot(right).abs())
            .fold(0.0, f32::max);
        let how = if aimed { "aimed" } else { "from the hip" };
        let what = format!(
            "{how}: the support hand is {rest:.3} m from its grip before the shot and at most {support:.4} m further during it; the shoulder goes {back:.4} m back and {up:.4} m up; the muzzle {along:.4} m back along the barrel, {lift:.4} m up and at most {sideways:.4} m sideways"
        );
        failed |= support > SUPPORT_MOST
            || back < 0.003
            || up > back / 2.0
            || along <= lift.abs()
            || sideways > SIDEWAYS_MOST * along;
        seen.push(what);
    }
    check(!failed, seen.join("; "))
}

/// Carth's head on the body posed as `globals`, as `cast::Head::turn` holds
/// it while the rifle is raised: upright and heading along the level way
/// to `target`; turned where it hangs (`headhook`) by the cheek weld
/// (`aim_pose::weld_turn`); turned by the least turn that makes it look at
/// `target` from where it turns (`hturn_g`); then leaning
/// `aim_pose::head_tilt_deg` toward the gun about that look; moved back by
/// `back` (metres), as far as the shot pushes the gun. The points of its
/// mesh, and its right eye.
fn welded_head(
    scene: &Scene,
    globals: &[(Vec3, Quat)],
    (target, back): (Vec3, f32),
) -> Result<(Vec<Vec3>, Vec3), String> {
    let hook = scene.node("headhook")?;
    let (at, turn) = globals[hook];
    let at = at - ((target - at) * Vec3::new(1.0, 1.0, 0.0)).normalize_or_zero() * back;
    let rest = rest_turn(scene, hook);
    let level = ((target - at) * Vec3::new(1.0, 1.0, 0.0)).normalize_or(Vec3::Y);
    // Upright, heading along the level way: the head model's space as it
    // stands at rest, turned about the up axis.
    let upright = Quat::from_rotation_arc(Vec3::Y, level) * rest;
    let pivot = at + turn * scene.head_turns_at;
    let weld = crate::aim_pose::cheek_weld_deg(Wield::Rifle).map_or(Quat::IDENTITY, |weld| {
        crate::aim_pose::weld_turn(target - pivot, weld, 1.0)
    });
    let pivot = at + weld * turn * scene.head_turns_at;
    let welded = weld * upright;
    let facing = welded * rest.inverse() * Vec3::Y;
    let look = (target - pivot).normalize_or(Vec3::Y);
    let tilt = crate::aim_pose::head_tilt_deg(Wield::Rifle).to_radians();
    let held = Quat::from_axis_angle(look, tilt) * Quat::from_rotation_arc(facing, look) * welded;
    let place = |point: Vec3| pivot + held * (point - scene.head_turns_at);
    Ok((
        scene.head.iter().map(|&point| place(point)).collect(),
        place(scene.right_eye),
    ))
}

/// Node `node`'s turn in the model's space at rest.
fn rest_turn(scene: &Scene, node: usize) -> Quat {
    let rest = &scene.body.rest;
    let mut turn = Quat::IDENTITY;
    let mut at = Some(node);
    while let Some(index) = at {
        turn = rest[index].2 * turn;
        at = rest[index].0;
    }
    turn
}

/// Each node's place in the model's space at rest (position, rotation).
fn rest_places(rest: &[(Option<usize>, Vec3, Quat)]) -> Vec<(Vec3, Quat)> {
    let mut places: Vec<Option<(Vec3, Quat)>> = vec![None; rest.len()];
    fn of(
        node: usize,
        rest: &[(Option<usize>, Vec3, Quat)],
        places: &mut [Option<(Vec3, Quat)>],
    ) -> (Vec3, Quat) {
        if let Some(done) = places[node] {
            return done;
        }
        let (parent, at, turn) = rest[node];
        let done = match parent {
            Some(parent) => {
                let (p_at, p_turn) = of(parent, rest, places);
                (p_at + p_turn * at, p_turn * turn)
            }
            None => (at, turn),
        };
        places[node] = Some(done);
        done
    }
    (0..rest.len())
        .map(|node| of(node, rest, &mut places))
        .collect()
}

/// The nearest point to `p` on the triangle `a`, `b`, `c` (Ericson's
/// closest point on a triangle).
fn on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denominator = 1.0 / (va + vb + vc);
    a + ab * (vb * denominator) + ac * (vc * denominator)
}

/// How far (metres) the vertices of the body's mesh moved by `limb`'s
/// nodes stand outside the surface of the mesh moved by `torso`'s nodes,
/// on the body posed as `globals`: the least, below 0 when a vertex is
/// inside. Each vertex is measured from the nearest point of that surface,
/// outside along the mesh's normal there (`normal_at`). The surface is not
/// closed where it meets the arms and the neck, so a vertex nearest one of
/// its open edges counts as outside. With `beyond`, only the limb's
/// vertices further than that (metres) from the first limb node's joint
/// count.
fn limb_clearance(
    scene: &Scene,
    globals: &[(Vec3, Quat)],
    (limb, torso): (&[usize], &[usize]),
    beyond: f32,
) -> f32 {
    let skin = &scene.skin;
    let posed = skin.posed(globals);
    let of = |nodes: &[usize], index: usize| nodes.contains(&skin.vertices[index].node);
    let joint = globals[limb[0]].0;
    let shown: Vec<[usize; 3]> = (skin.triangles.iter().copied())
        .filter(|corners| corners.iter().all(|&index| of(torso, index)))
        .collect();
    // An edge is open when only one triangle has it. Vertices split for
    // their texture stand at the same place, so edges go by place at rest.
    let key = |index: usize| {
        (skin.vertices[index].point * 1e4)
            .round()
            .as_ivec3()
            .to_array()
    };
    let edge = |a: usize, b: usize| {
        let (a, b) = (key(a), key(b));
        if a < b { (a, b) } else { (b, a) }
    };
    let mut uses = std::collections::HashMap::new();
    for &[a, b, c] in &shown {
        for (p, q) in [(b, c), (c, a), (a, b)] {
            *uses.entry(edge(p, q)).or_insert(0) += 1;
        }
    }
    let walls: Vec<([Vec3; 3], [Vec3; 3], [bool; 3])> = (shown.iter())
        .map(|&[a, b, c]| {
            let open = [(b, c), (c, a), (a, b)].map(|(p, q)| uses[&edge(p, q)] == 1);
            (
                [a, b, c].map(|i| posed[i].0),
                [a, b, c].map(|i| posed[i].1),
                open,
            )
        })
        .collect();
    (0..posed.len())
        .filter(|&index| of(limb, index) && posed[index].0.distance(joint) > beyond)
        .filter_map(|index| {
            let point = posed[index].0;
            (walls.iter())
                .map(|&(corners, normals, open)| {
                    let near = on_triangle(point, corners);
                    (near.distance_squared(point), near, (corners, normals, open))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(squared, near, (corners, normals, open))| {
                    let weights = barycentric(near, corners);
                    let edged = (0..3).any(|k| open[k] && weights[k] < 1e-4);
                    let out =
                        normals[0] * weights[0] + normals[1] * weights[1] + normals[2] * weights[2];
                    let side = if edged { 1.0 } else { (point - near).dot(out) };
                    squared.sqrt().copysign(side)
                })
        })
        .fold(f32::INFINITY, f32::min)
}

/// The barycentric weights of `near` on the triangle `a`, `b`, `c`: each
/// corner's, 0 on the edge across from it.
fn barycentric(near: Vec3, [a, b, c]: [Vec3; 3]) -> [f32; 3] {
    let face = (b - a).cross(c - a);
    let whole = face.length_squared();
    if whole <= 0.0 {
        return [1.0 / 3.0; 3];
    }
    let share = |p: Vec3, q: Vec3| (q - p).cross(near - p).dot(face) / whole;
    [share(b, c), share(c, a), share(a, b)]
}

/// The least clearance (metres) a forearm keeps from the chest. With the
/// elbow out to the side at about shoulder height, the trigger hand's wrist
/// sinks into the chest: up to 0.6 cm aimed level and 1.5 cm aimed 10
/// degrees down, where the chest bows toward the target.
const FOREARM_CLEAR: f32 = -0.016;
/// The least distance (metres) between the arms and the head.
const ARM_FROM_HEAD: f32 = 0.01;
/// How far (metres) from the shoulder joint the upper arm is measured from
/// the chest: the elbow half of it, since the arm meets the body at the
/// armpit.
const UPPER_ARM_FROM: f32 = 0.12;
/// Every how many moments the forearms are measured through a shot.
const CLEAR_EVERY: usize = 3;

/// A fully raised blaster rifle on Carth's body aimed level, 10 degrees up
/// and 10 degrees down, firing once: each forearm stays at least
/// `FOREARM_CLEAR` outside the chest's mesh, and the arms at least
/// `ARM_FROM_HEAD` from the head resting on the stock (`welded_head`). The
/// elbow half of each upper arm, and how high the right elbow stands above
/// the shoulder joint, are measured but not checked.
fn forearms_clear(scene: &Scene) -> Result<String, String> {
    let node = |name: &str| scene.node(name);
    let torso = [
        node("torso_g")?,
        node("torsoupr_g")?,
        node("rcollar_g")?,
        node("lcollar_g")?,
    ];
    let chest = &torso[..2];
    let arms = [
        (
            "right",
            [node("rbicep_g")?, node("rbicepl_g")?, node("rforearm_g")?],
        ),
        (
            "left",
            [node("lbicep_g")?, node("lbicepl_g")?, node("lforearm_g")?],
        ),
    ];
    let pose = scene.ja_pose(Wield::Rifle, true)?;
    let (mut seen, mut failed) = (Vec::new(), false);
    for pitch in [0.0, 10.0, -10.0] {
        let shooter = Shooter::new(scene, (Wield::Rifle, Blaster::Rifle), &pose)?.pitched(pitch);
        let target = shooter.targets[0];
        let firing = shooter.firing(Some(0.0), 1.0);
        let (moments, ..) =
            fired_pitched(scene, (Wield::Rifle, Blaster::Rifle, 1.0), &pose, pitch)?;
        let mut lines = Vec::new();
        for (side, [upper, lower, fore]) in arms {
            let (mut forearm, mut elbow_half) = (f32::INFINITY, f32::INFINITY);
            for (_, globals) in moments.iter().step_by(CLEAR_EVERY) {
                forearm = forearm.min(limb_clearance(scene, globals, (&[fore], &torso), 0.0));
                elbow_half = elbow_half.min(limb_clearance(
                    scene,
                    globals,
                    (&[upper, lower], chest),
                    UPPER_ARM_FROM,
                ));
            }
            failed |= forearm < FOREARM_CLEAR;
            lines.push(format!(
                "the {side} forearm {forearm:.4} m and the elbow half of the upper arm {elbow_half:.4} m"
            ));
        }
        let skin = &scene.skin;
        let mut head_gap = f32::INFINITY;
        for (now, globals) in moments.iter().step_by(CLEAR_EVERY) {
            let back = push(&firing, 0, *now).back_m;
            let (head, _) = welded_head(scene, globals, (target, back))?;
            let posed = skin.posed(globals);
            let arm_points = (0..posed.len())
                .filter(|&index| {
                    (arms.iter()).any(|(_, bones)| bones.contains(&skin.vertices[index].node))
                })
                .map(|index| posed[index].0);
            for point in arm_points {
                head_gap = (head.iter().map(|h| h.distance(point))).fold(head_gap, f32::min);
            }
        }
        failed |= head_gap < ARM_FROM_HEAD;
        let before = &moments[0].1;
        let upper_arm = before[node("rforearm_g")?].0 - before[node("rbicep_g")?].0;
        let elbow = upper_arm.z;
        let from_down = upper_arm.angle_between(Vec3::NEG_Z).to_degrees();
        seen.push(format!(
            "aimed {pitch}° up: outside the chest by at least {}; the arms at least {head_gap:.4} m from the head; the right elbow {elbow:.4} m above the shoulder joint, its upper arm {from_down:.1}° from hanging straight down",
            lines.join(" and ")
        ));
    }
    check(!failed, seen.join("; "))
}

/// The least distance (metres) between a point of `head` and a point of
/// the rifle in `globals`.
fn gap(scene: &Scene, globals: &[(Vec3, Quat)], head: &[Vec3]) -> Result<f32, String> {
    let (at, turn) = globals[scene.node("rhand")?];
    let rifle: Vec<Vec3> = (scene.rifle_points.iter())
        .map(|&point| at + turn * point)
        .collect();
    Ok((head.iter())
        .flat_map(|h| rifle.iter().map(move |r| h.distance(*r)))
        .fold(f32::INFINITY, f32::min))
}

/// Where the right eye stands from the rifle in `globals`: how far (metres)
/// above its top it is at the eye's place along the barrel, and how far to
/// the side of the upright plane along the barrel through its middle.
fn eye_on_sight(scene: &Scene, globals: &[(Vec3, Quat)], eye: Vec3) -> Result<(f32, f32), String> {
    let (at, turn) = globals[scene.node("rhand")?];
    let barrel = (turn * scene.rifle.barrel).normalize_or_zero();
    let side = barrel.cross(Vec3::Z).normalize_or_zero();
    let middle = at + turn * scene.rifle.foregrip(RIFLE_SUPPORT);
    let top = (scene.rifle_points.iter())
        .map(|&point| at + turn * point)
        .filter(|point| (point - eye).dot(barrel).abs() < 0.05)
        .map(|point| point.z)
        .fold(f32::NEG_INFINITY, f32::max);
    Ok((eye.z - top, (eye - middle).dot(side)))
}

/// A fully raised rifle firing, a blaster rifle and a disruptor rifle (the
/// largest kick), aimed level, 10 degrees up and 10 degrees down: the head rests its cheek on the stock (`welded_head`), so
/// it comes within `CHEEK_NEAR` of the rifle before the shot, but the rifle
/// always stays at least `HEAD_CLEAR` from it. The right eye stands above
/// the rifle, within `EYE_ASIDE` of the upright plane along its barrel.
fn head_clear(scene: &Scene) -> Result<String, String> {
    let pose = scene.ja_pose(Wield::Rifle, true)?;
    let (mut seen, mut failed) = (Vec::new(), false);
    let cases = [Blaster::Rifle, Blaster::Disruptor]
        .into_iter()
        .flat_map(|blaster| [0.0, 10.0, -10.0].map(|pitch| (blaster, pitch)));
    for (blaster, pitch) in cases {
        let shooter = Shooter::new(scene, (Wield::Rifle, blaster), &pose)?.pitched(pitch);
        let target = shooter.targets[0];
        let firing = shooter.firing(Some(0.0), 1.0);
        let (moments, ..) = fired_pitched(scene, (Wield::Rifle, blaster, 1.0), &pose, pitch)?;
        let (head, eye) = welded_head(scene, &moments[0].1, (target, 0.0))?;
        let before = gap(scene, &moments[0].1, &head)?;
        let (above, aside) = eye_on_sight(scene, &moments[0].1, eye)?;
        let mut nearest = f32::INFINITY;
        for (now, globals) in &moments {
            let back = push(&firing, 0, *now).back_m;
            let (head, _) = welded_head(scene, globals, (target, back))?;
            nearest = nearest.min(gap(scene, globals, &head)?);
        }
        failed |=
            nearest < HEAD_CLEAR || before > CHEEK_NEAR || aside.abs() > EYE_ASIDE || above <= 0.0;
        seen.push(format!(
            "{blaster:?} aimed {pitch}° up: {before:.4} m from the head before the shot, at least {nearest:.4} m during it; the right eye {above:.4} m above the rifle and {aside:.4} m to the right of its barrel"
        ));
    }
    check(!failed, seen.join("; "))
}

/// The most (metres) a palm may stand from where it holds the rifle.
const PALM_ON_GRIP: f32 = 0.01;

/// A raised rifle aimed level, 10 degrees up and 10 degrees down, each
/// firing once: the support hand's palm (`aim_pose::support_palm`) stays
/// within `PALM_ON_GRIP` of where it holds the gun all through the shot.
/// A rifle held at the hip is measured too, but not checked: JA's pose
/// there holds the gun further out than the left arm reaches.
fn support_on_grip(scene: &Scene) -> Result<String, String> {
    let (gun, wrist) = (scene.node("rhand")?, scene.node("lhand_g")?);
    let fingers = |end: char| -> Result<Vec<usize>, String> {
        ['a', 'b', 'c', 'd']
            .iter()
            .map(|finger| scene.node(&format!("l{finger}fngr{end}_g")))
            .collect()
    };
    let (bases, tips) = (fingers('b')?, fingers('t')?);
    let grip = scene.rifle.foregrip(RIFLE_SUPPORT);
    let palm_gap = |globals: &[(Vec3, Quat)]| {
        let mean = |nodes: &[usize]| nodes.iter().map(|&n| globals[n].0).sum::<Vec3>() / 4.0;
        let palm = crate::aim_pose::support_palm(globals[wrist].0, mean(&bases), mean(&tips));
        let (at, turn) = globals[gun];
        palm.distance(at + turn * grip)
    };
    let (mut seen, mut failed) = (Vec::new(), false);
    for (aimed, pitch) in [(true, 0.0), (true, 10.0), (true, -10.0), (false, 0.0)] {
        let pose = scene.ja_pose(Wield::Rifle, aimed)?;
        let raise = if aimed { 1.0 } else { 0.0 };
        let (moments, ..) =
            fired_pitched(scene, (Wield::Rifle, Blaster::Rifle, raise), &pose, pitch)?;
        let gaps: Vec<f32> = moments
            .iter()
            .map(|(_, globals)| palm_gap(globals))
            .collect();
        let most = gaps.iter().copied().fold(0.0, f32::max);
        failed |= aimed && most > PALM_ON_GRIP;
        let how = if aimed { "raised" } else { "at the hip" };
        seen.push(format!(
            "{how}, {pitch}° up: {:.4} m before the shot, at most {most:.4} m",
            gaps[0]
        ));
    }
    check(!failed, seen.join("; "))
}

/// How far (metres) the rear end of a raised rifle's stock may stand above
/// or below the gun-side shoulder joint.
const STOCK_HEIGHT: (f32, f32) = (-0.03, 0.03);
/// The least distance (metres) between any point of a raised rifle and the
/// gun-side shoulder joint or the bone of the upper arm, so that the rifle
/// does not pass into the shoulder.
const SHOULDER_CLEAR: f32 = 0.08;
/// The least share of the stock's way back that the gun-side shoulder joint
/// goes back with it at the peak of a raised rifle's shot.
const SHOULDER_FOLLOWS: f32 = 0.6;

/// How deep (metres) along the barrel the rear end of a rifle's stock is
/// taken to be.
const BUTT: f32 = 0.015;

/// The least distance from `point` to the segment from `a` to `b`.
fn to_segment(point: Vec3, (a, b): (Vec3, Vec3)) -> f32 {
    let along = (b - a).length_squared();
    let t = if along > 0.0 {
        ((point - a).dot(b - a) / along).clamp(0.0, 1.0)
    } else {
        0.0
    };
    point.distance(a + (b - a) * t)
}

/// A fully raised blaster rifle on Carth's body: the middle of the rear end
/// of its stock stands within `STOCK_HEIGHT` of the height of the gun-side
/// shoulder joint and in front of it, and every point of the rifle stays at
/// least `SHOULDER_CLEAR` from that joint and from the upper arm's bone,
/// before and all through a shot. At the shot's peak the shoulder joint
/// goes back at least `SHOULDER_FOLLOWS` as far as the stock does, so the
/// body takes the kick with the gun.
fn rifle_at_shoulder(scene: &Scene) -> Result<String, String> {
    let (gun, shoulder, elbow) = (
        scene.node("rhand")?,
        scene.node("rbicep_g")?,
        scene.node("rforearm_g")?,
    );
    let pose = scene.ja_pose(Wield::Rifle, true)?;
    let (moments, right, forward) = fired(scene, (Wield::Rifle, Blaster::Rifle, 1.0), &pose)?;
    // The middle of the stock's rear end: the points within `BUTT` of the
    // rearmost along the barrel.
    let along = |point: &Vec3| point.dot(scene.rifle.barrel);
    let last = (scene.rifle_points.iter().map(along)).fold(f32::INFINITY, f32::min);
    let butt: Vec<Vec3> = (scene.rifle_points.iter().copied())
        .filter(|point| along(point) <= last + BUTT)
        .collect();
    let rear = butt.iter().sum::<Vec3>() / butt.len().max(1) as f32;
    let stock = |globals: &[(Vec3, Quat)]| {
        let (at, turn) = globals[gun];
        at + turn * rear
    };
    let clearance = |globals: &[(Vec3, Quat)]| {
        let (at, turn) = globals[gun];
        let bone = (globals[shoulder].0, globals[elbow].0);
        (scene.rifle_points.iter())
            .map(|&point| at + turn * point)
            .map(|point| point.distance(bone.0).min(to_segment(point, bone)))
            .fold(f32::INFINITY, f32::min)
    };
    let before = &moments[0].1;
    let off = stock(before) - before[shoulder].0;
    let (ahead, up, aside) = (off.dot(forward), off.z, off.dot(right));
    let nearest = (moments.iter())
        .map(|(_, globals)| clearance(globals))
        .fold(f32::INFINITY, f32::min);
    let peak = at(
        &moments,
        f64::from(crate::weapons::handling(Blaster::Rifle).rise_ms),
    );
    let back = |point: Vec3, was: Vec3| -(point - was).dot(forward);
    let stock_back = back(stock(peak), stock(before));
    let shoulder_back = back(peak[shoulder].0, before[shoulder].0);
    // The same, square to the line between the shoulders, where the chest
    // faces.
    let across =
        (before[shoulder].0 - before[scene.node("lbicep_g")?].0) * Vec3::new(1.0, 1.0, 0.0);
    let facing = Vec3::Z.cross(across).normalize_or(forward);
    let square = |point: Vec3, was: Vec3| -(point - was).dot(facing);
    let (stock_square, shoulder_square) = (
        square(stock(peak), stock(before)),
        square(peak[shoulder].0, before[shoulder].0),
    );
    check(
        (STOCK_HEIGHT.0..=STOCK_HEIGHT.1).contains(&up)
            && ahead > 0.0
            && nearest >= SHOULDER_CLEAR
            && shoulder_back >= SHOULDER_FOLLOWS * stock_back,
        format!(
            "the stock's rear end is {ahead:.3} m in front of the shoulder joint, {up:.3} m above it and {aside:.3} m to its right; the rifle comes at least {nearest:.3} m from the shoulder joint and the upper arm; at the shot's peak the stock goes {stock_back:.4} m back along the aim and the shoulder joint {shoulder_back:.4} m; square to the shoulders {stock_square:.4} m and {shoulder_square:.4} m"
        ),
    )
}

/// When the rifle fires (sim ms) in the runs that press or let go of [Aim]
/// around its shot.
const TOGGLE_SHOT: u64 = 1000;
/// When [Aim] is pressed in the runs that let go of it: early enough for
/// the gun to be fully raised by the shot.
const HELD_FROM: u64 = TOGGLE_SHOT - 600;
/// The moments (ms from the shot) when [Aim] is pressed or let go of.
const TOGGLES: [i64; 10] = [-160, -80, -40, 0, 16, 40, 80, 120, 200, 320];
/// How long after the shot each run lasts (ms).
const TOGGLE_AFTER: u64 = 800;

/// The right hand's gun at each step of a rifle run on Carth's body: its
/// muzzle (metres, the model's space), with the firing motion and without
/// it. [Aim] is pressed at `aim_from` and let go of at `release`, the
/// rifle fires at `TOGGLE_SHOT`, and the run is measured from 200 ms
/// before the shot.
fn toggled(
    scene: &Scene,
    hip: &[asset_ja::g2::Local],
    (aim_from, release): (u64, u64),
) -> Result<[Vec<Vec3>; 2], String> {
    use crate::aim_pose_probes::{Step, aimed, firing};
    let ja = &scene.ja;
    let torso = firing(ja, Wield::Rifle, &[TOGGLE_SHOT])?;
    let step = |now: u64| -> Result<Step, String> {
        Ok(Step {
            legs: ja.part(sim::motion::Anim::BOTH_STAND1, 0, true)?,
            torso: torso(now),
            shooting: aimed(now, aim_from, release),
        })
    };
    let frames = run(
        ja,
        (Wield::Rifle, true, 0..=TOGGLE_SHOT + TOGGLE_AFTER),
        step,
        |figure, now| (now, figure.pose(now as f64), figure.raise_share(now as f64)),
    )?;
    let gun = scene.node("rhand")?;
    let muzzle = scene.rifle.muzzle;
    let mut paths = [Vec::new(), Vec::new()];
    for (path, moving) in paths.iter_mut().zip([true, false]) {
        let mut shooter = Shooter::new(scene, (Wield::Rifle, Blaster::Rifle), hip)?;
        for (now, pose, raise) in frames.iter().filter(|f| f.0 + 200 >= TOGGLE_SHOT) {
            let shot = (*now >= TOGGLE_SHOT).then_some(TOGGLE_SHOT as f64);
            let firing = shooter.firing(shot, *raise);
            let globals = shooter.at(pose, &firing, (*now as f64, moving));
            let (at, turn) = globals[gun];
            path.push(at + turn * muzzle);
        }
    }
    Ok(paths)
}

/// The largest distance (metres) between two neighbouring points, and the
/// index of the later one.
fn largest_step(points: &[Vec3]) -> (f32, usize) {
    (points.windows(2).enumerate())
        .map(|(at, pair)| (pair[1].distance(pair[0]), at + 1))
        .fold(
            (0.0, 0),
            |most, step| if step.0 > most.0 { step } else { most },
        )
}

/// The largest step (metres) the firing motion alone may move the rifle's
/// muzzle while [Aim] is pressed or let go of: a quarter of a blaster
/// rifle's push back at the hip.
const OWN_STEP_MOST: f32 = HIP_RIFLE_PUSH.1 / 4.0;

/// [Aim] pressed at the hip, or let go of when fully raised, at each of
/// `TOGGLES` around a rifle shot on Carth's body. The rifle's muzzle is
/// followed with the firing motion and without it, and the firing motion's
/// own part is the difference of the two. No step of that part moves the
/// muzzle more than `OWN_STEP_MOST`. Without the firing motion, no step
/// moves the muzzle further than the aim pose allows a step of the
/// let-down (`aim_pose_probes::LOWER_MOST` of the way from the hip to the
/// full raise). Each run's largest step with the firing motion is given
/// too.
fn aim_toggles(scene: &Scene) -> Result<String, String> {
    let hip = scene.ja_pose(Wield::Rifle, false)?;
    let full = scene.ja_pose(Wield::Rifle, true)?;
    let gun = scene.node("rhand")?;
    let mut shooter = Shooter::new(scene, (Wield::Rifle, Blaster::Rifle), &hip)?;
    let unfired = shooter.firing(None, 0.0);
    let muzzle = |pose: &[asset_ja::g2::Local], shooter: &mut Shooter| {
        let (at, turn) = shooter.at(pose, &unfired, (0.0, false))[gun];
        at + turn * scene.rifle.muzzle
    };
    let travel = muzzle(&full, &mut shooter).distance(muzzle(&hip, &mut shooter));
    let most = crate::aim_pose_probes::LOWER_MOST * travel;
    let (mut seen, mut worst) = (Vec::new(), [0.0f32; 2]);
    for pressed in [true, false] {
        for moment in TOGGLES {
            let at = TOGGLE_SHOT.saturating_add_signed(moment);
            let times = if pressed {
                (at, NEVER)
            } else {
                (HELD_FROM, at)
            };
            let [moving, still] = toggled(scene, &hip, times)?;
            let own: Vec<Vec3> = moving.iter().zip(&still).map(|(a, b)| *a - *b).collect();
            let ((step, index), (without, _), (alone, _)) = (
                largest_step(&moving),
                largest_step(&still),
                largest_step(&own),
            );
            worst = [worst[0].max(alone), worst[1].max(without)];
            let when = index as i64 * STEP_MS as i64 - 200;
            let how = if pressed { "pressed" } else { "let go" };
            seen.push(format!(
                "{how} at {moment} ms: {step:.4} m at {when} ms, {without:.4} m without the firing motion, {alone:.4} m by the firing motion alone"
            ));
        }
    }
    check(
        worst[0] <= OWN_STEP_MOST && worst[1] <= most,
        format!(
            "the largest step by the firing motion alone {:.4} m (at most {OWN_STEP_MOST:.4} m), without it {:.4} m (at most {most:.4} m, {travel:.3} m from the hip to the full raise); {}",
            worst[0],
            worst[1],
            seen.join("; ")
        ),
    )
}

/// The shots of the breathing check (sim ms): a rifle from the hip, 1 s
/// apart.
const BREATH_SHOTS: [u64; 3] = [1000, 2000, 3000];

/// A rifle fired from the hip every second still breathes between its
/// shots as a body that does not fire. Its breathing (`stance::Breath`) is
/// the same at every step, and from the end of each shot's settle time
/// until the next shot the firing motion adds no turn and no push of the
/// shoulder over it.
fn breathes(scene: &Scene) -> Result<String, String> {
    use crate::aim_pose_probes::{Step, firing};
    let ja = &scene.ja;
    let end = BREATH_SHOTS[2] + 1000;
    let breath = |shots: &[u64]| -> Result<Vec<(f32, f32)>, String> {
        let torso = firing(ja, Wield::Rifle, shots)?;
        let step = |now: u64| -> Result<Step, String> {
            Ok(Step {
                legs: ja.part(sim::motion::Anim::BOTH_STAND1, 0, true)?,
                torso: torso(now),
                shooting: Default::default(),
            })
        };
        // The breathing's clips are not needed: its weight and lean show
        // whether it breathes.
        let clips = Vec::new();
        let mut breath = crate::stance::Breath::default();
        breath.wield = 7;
        run(ja, (Wield::Rifle, true, 0..=end), step, |figure, now| {
            let shown = breath.follow(&clips, figure.holding(), (now as f64, 0.0));
            (breath.weight(), shown.map_or(0.0, |shown| shown.lean))
        })
    };
    let (fires, still) = (breath(&BREATH_SHOTS)?, breath(&[])?);
    let unlike = (fires.iter().zip(&still).position(|(a, b)| a != b))
        .map(|step| step as u64 * STEP_MS as u64);
    let shooter = Shooter::new(
        scene,
        (Wield::Rifle, Blaster::Rifle),
        &scene.ja_pose(Wield::Rifle, false)?,
    )?;
    let settled = (RISE_MS + RIFLE_SETTLE_MS) as u64;
    let mut moved = None;
    let mut sway = (f32::INFINITY, f32::NEG_INFINITY);
    for (index, &shot) in BREATH_SHOTS.iter().enumerate() {
        let next = BREATH_SHOTS.get(index + 1).copied().unwrap_or(end);
        let firing = shooter.firing(Some(shot as f64), 0.0);
        for now in (shot + settled..next).step_by(STEP_MS as usize) {
            let ms = now as f64;
            if moved.is_none()
                && (!turns(&firing, ms).is_empty() || push(&firing, 0, ms).shoulder_deg != 0.0)
            {
                moved = Some(now);
            }
            let lean = fires[(now / STEP_MS as u64) as usize].1;
            sway = (sway.0.min(lean), sway.1.max(lean));
        }
    }
    let sways = sway.1 - sway.0 > 0.0;
    check(
        unlike.is_none() && moved.is_none() && sways,
        format!(
            "the breathing differs from a body that does not fire at {unlike:?} ms; the firing motion moves the body between shots at {moved:?} ms; the breathing's lean between shots goes from {:.3}° to {:.3}°",
            sway.0.to_degrees(),
            sway.1.to_degrees()
        ),
    )
}

/// A pair aimed, the right gun firing: the left gun moves no more than
/// `STILL_MOST`, and the right one at least 2 cm.
fn pair(scene: &Scene) -> Result<String, String> {
    let weapons = [scene.node("rhand")?, scene.node("lhand")?];
    let pose = scene.ja_pose(Wield::Pair, true)?;
    let (moments, ..) = fired(scene, (Wield::Pair, Blaster::Pistol, 1.0), &pose)?;
    let [right, left] =
        weapons.map(|node| drift(moments.iter().map(|(_, globals)| globals[node].0)));
    check(
        left <= STILL_MOST && right >= 0.02,
        format!("the right gun moves {right:.4} m, the left {left:.4} m"),
    )
}

pub(super) fn checks() -> sim::probe::Checks {
    let names = [
        "rifle_into_shoulder",
        "pair_other_gun_still",
        "raised_rifle_clears_face",
        "aim_toggle_during_shot",
        "breathing_between_shots",
        "raised_rifle_at_shoulder",
        "support_hand_on_grip",
        "raised_rifle_forearms_clear_chest",
    ];
    match Scene::load() {
        Ok(scene) => vec![
            (names[0], rifle(&scene)),
            (names[1], pair(&scene)),
            (names[2], head_clear(&scene)),
            (names[3], aim_toggles(&scene)),
            (names[4], breathes(&scene)),
            (names[5], rifle_at_shoulder(&scene)),
            (names[6], support_on_grip(&scene)),
            (names[7], forearms_clear(&scene)),
        ],
        Err(why) => names
            .map(|name| (name, Ok(format!("skipped: {why}"))))
            .into(),
    }
}
