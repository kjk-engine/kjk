//! Probes of the ragdolls on bodies made here (no game data): the approved
//! test `ragdoll_physics` runs them all.

use std::sync::Arc;

use glam::{Quat, Vec3};

use crate::cut::{Cut, SkinnedMesh, Wound};
use crate::{Frame, Material, Muscles, Node, Skeleton, Template, Vertex, World, humanoid};

/// One probe: what it checks, what it measured, whether it passed.
#[derive(Debug, Clone)]
pub struct Probe {
    pub name: &'static str,
    pub expect: &'static str,
    pub measured: String,
    pub pass: bool,
}

const STEP: f32 = 1.0 / 120.0;

/// A standing stick figure with KOTOR's humanoid node names (metres, facing
/// +Y, +X its right).
fn figure() -> Vec<Node> {
    let nodes: [(&str, Option<usize>, [f32; 3]); 17] = [
        ("pelvis_g", None, [0.0, 0.0, 1.0]),
        ("lthigh_g", Some(0), [-0.1, 0.0, 0.95]),
        ("lshin_g", Some(1), [-0.1, 0.0, 0.5]),
        ("lfoot_g", Some(2), [-0.1, 0.0, 0.08]),
        ("rthigh_g", Some(0), [0.1, 0.0, 0.95]),
        ("rshin_g", Some(4), [0.1, 0.0, 0.5]),
        ("rfoot_g", Some(5), [0.1, 0.0, 0.08]),
        ("torso_g", Some(0), [0.0, 0.0, 1.1]),
        ("torsoupr_g", Some(7), [0.0, 0.0, 1.3]),
        ("neck_g", Some(8), [0.0, 0.0, 1.5]),
        ("head_g", Some(9), [0.0, 0.0, 1.58]),
        ("lbicep_g", Some(8), [-0.2, 0.0, 1.45]),
        ("lforearm_g", Some(11), [-0.48, 0.0, 1.45]),
        ("lhand_g", Some(12), [-0.74, 0.0, 1.45]),
        ("rbicep_g", Some(8), [0.2, 0.0, 1.45]),
        ("rforearm_g", Some(14), [0.48, 0.0, 1.45]),
        ("rhand_g", Some(15), [0.74, 0.0, 1.45]),
    ];
    nodes
        .into_iter()
        .map(|(name, parent, at)| Node {
            name: name.into(),
            parent,
            rest: Frame::new(Vec3::from_array(at), Quat::IDENTITY),
        })
        .collect()
}

/// A walker droid: a body box on two legs of two pieces each, its mesh
/// rigid on its nodes.
fn droid() -> (Vec<Node>, Vec<Vertex>) {
    let nodes: [(&str, Option<usize>, [f32; 3]); 5] = [
        ("body", None, [0.0, 0.0, 1.0]),
        ("lthigh", Some(0), [-0.25, 0.0, 0.9]),
        ("lshin", Some(1), [-0.25, 0.0, 0.45]),
        ("rthigh", Some(0), [0.25, 0.0, 0.9]),
        ("rshin", Some(3), [0.25, 0.0, 0.45]),
    ];
    let boxes: [(usize, [f32; 3], [f32; 3]); 5] = [
        (0, [-0.3, -0.25, 0.95], [0.3, 0.25, 1.4]),
        (1, [-0.3, -0.05, 0.5], [-0.2, 0.05, 0.9]),
        (2, [-0.3, -0.08, 0.0], [-0.2, 0.12, 0.45]),
        (3, [0.2, -0.05, 0.5], [0.3, 0.05, 0.9]),
        (4, [0.2, -0.08, 0.0], [0.3, 0.12, 0.45]),
    ];
    let mut skin = Vec::new();
    for (node, min, max) in boxes {
        // A lattice through each box.
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let t = Vec3::new(i as f32, j as f32, k as f32) / 3.0;
                    let (min, max) = (Vec3::from_array(min), Vec3::from_array(max));
                    skin.push(Vertex::rigid(min + (max - min) * t, node));
                }
            }
        }
    }
    let nodes = nodes
        .into_iter()
        .map(|(name, parent, at)| Node {
            name: name.into(),
            parent,
            rest: Frame::new(Vec3::from_array(at), Quat::IDENTITY),
        })
        .collect();
    (nodes, skin)
}

/// A defense turret like Kashyyyk's Czerka ones: a root node with no mesh
/// of its own, four small plates on it, a heavy base and a heavy top, and a
/// thin barrel on the top. Its mesh is rigid on its nodes.
fn turret() -> (Vec<Node>, Vec<Vertex>) {
    let nodes: [(&str, Option<usize>, [f32; 3]); 8] = [
        ("root", None, [0.0, 0.0, 0.0]),
        ("plate1", Some(0), [0.0, -0.35, 0.2]),
        ("plate2", Some(0), [0.0, 0.35, 0.2]),
        ("plate3", Some(0), [0.35, 0.0, 0.2]),
        ("plate4", Some(0), [-0.35, 0.0, 0.2]),
        ("base", Some(0), [0.02, 0.01, 0.0]),
        ("top", Some(5), [0.02, 0.0, 0.54]),
        ("barrel", Some(6), [0.01, 0.69, 0.73]),
    ];
    let boxes: [(usize, [f32; 3], [f32; 3]); 7] = [
        (1, [-0.2, -0.55, 0.15], [0.2, -0.35, 0.25]),
        (2, [-0.2, 0.35, 0.15], [0.2, 0.55, 0.25]),
        (3, [0.35, -0.2, 0.15], [0.55, 0.2, 0.25]),
        (4, [-0.55, -0.2, 0.15], [-0.35, 0.2, 0.25]),
        (5, [-0.57, -0.58, 0.19], [0.61, 0.6, 0.56]),
        (6, [-0.78, -0.08, 0.55], [0.82, 0.42, 1.04]),
        (7, [-0.03, 0.32, 0.69], [0.05, 1.06, 0.77]),
    ];
    let mut skin = Vec::new();
    for (node, min, max) in boxes {
        let (min, max) = (Vec3::from_array(min), Vec3::from_array(max));
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let t = Vec3::new(i as f32, j as f32, k as f32) / 3.0;
                    skin.push(Vertex::rigid(min + (max - min) * t, node));
                }
            }
        }
    }
    let nodes = nodes
        .into_iter()
        .map(|(name, parent, at)| Node {
            name: name.into(),
            parent,
            rest: Frame::new(Vec3::from_array(at), Quat::IDENTITY),
        })
        .collect();
    (nodes, skin)
}

/// A world with a floor at z 0.
fn floored() -> World {
    let mut world = World::new();
    let s = 20.0;
    let [a, b, c, d] = [[-s, -s], [s, -s], [s, s], [-s, s]].map(|[x, y]| Vec3::new(x, y, 0.0));
    world.add_faces([(0, [a, b, c]), (0, [a, c, d])]);
    world
}

fn rest_places(template: &Template) -> Vec<Frame> {
    template.parts.iter().map(|part| part.rest).collect()
}

fn part(template: &Template, name: &str) -> usize {
    template
        .parts
        .iter()
        .position(|p| p.name == name)
        .expect("the humanoid plan has the part")
}

/// Runs `world` for `seconds`, driving `id` to `targets` with `muscles`
/// when given.
fn run(world: &mut World, (id, seconds): (crate::Id, f32), drive: Option<(&[Frame], Muscles)>) {
    let steps = (seconds / STEP).round() as u32;
    for step in 0..steps {
        if let Some((targets, muscles)) = drive {
            world.drive(id, targets, muscles, f64::from(step) * f64::from(STEP));
        }
        world.step(STEP);
    }
}

/// Every probe, in order.
pub fn all() -> Vec<Probe> {
    let nodes = figure();
    let skeleton = Skeleton {
        nodes: &nodes,
        skin: &[],
    };
    let template = Arc::new(Template::fit(&humanoid::PLAN, &skeleton).expect("the figure fits"));
    let places = rest_places(&template);
    let mut probes = Vec::new();

    // A knee kicked forward or back bends back only; an elbow, forward only.
    for (name, part_name, push, toward, expect) in [
        (
            "knee",
            "shin left",
            Vec3::Y,
            -1.0,
            "a shin kicked forward or back folds behind the thigh",
        ),
        (
            "elbow",
            "forearm right",
            Vec3::NEG_Y,
            1.0,
            "a forearm pushed back or forward folds in front",
        ),
    ] {
        let mut folds = Vec::new();
        for sign in [1.0, -1.0] {
            let mut world = floored();
            let id = world.spawn(template.clone(), &places, Vec3::ZERO, false);
            let child = part(&template, part_name);
            let parent = template.parts[child]
                .parent
                .expect("a limb hangs on a part");
            let solid = template.parts[child].solid;
            let point = places[child].point(solid.centre() + solid.axis() * 0.15);
            world.strike(id, child, push * sign * 6.0, point);
            run(&mut world, (id, 0.25), None);
            let now = world.places(id).expect("the ragdoll is there");
            let bone = solid.axis();
            let local = now[parent].rotation.inverse() * (now[child].rotation * bone);
            folds.push(local.y * toward);
        }
        probes.push(Probe {
            name,
            expect,
            measured: format!("{folds:.2?} (each ≥ 0)"),
            pass: folds.iter().all(|&f| f >= -0.02),
        });
    }

    // Held to its own standing pose, a body stands.
    let mut world = floored();
    let id = world.spawn(template.clone(), &places, Vec3::ZERO, false);
    let held = Muscles {
        hold: 1.0,
        tone: 1.0,
    };
    run(&mut world, (id, 2.0), Some((&places, held)));
    let now = world.places(id).expect("the ragdoll is there");
    let (pelvis, head) = (
        now[0].translation.z,
        now[part(&template, "head")].translation.z,
    );
    probes.push(Probe {
        name: "stands",
        expect: "held to its standing pose for 2 s, the pelvis stays within 10 cm of its height and the head above 1.4 m",
        measured: format!("pelvis {pelvis:.2} m, head {head:.2} m"),
        pass: (pelvis - 1.0).abs() < 0.1 && head > 1.4,
    });

    // Let go with a push, a body falls, comes to rest and sleeps.
    let mut world = floored();
    let id = world.spawn(template.clone(), &places, Vec3::new(0.0, 2.0, 0.0), false);
    let mut slept = None;
    for step in 0..(5.0 / STEP) as u32 {
        world.step(STEP);
        if slept.is_none() && world.resting(id) {
            slept = Some(step as f32 * STEP);
        }
    }
    let now = world.places(id).expect("the ragdoll is there");
    let pelvis = now[0].translation.z;
    probes.push(Probe {
        name: "rests",
        expect: "a limp body pushed over lies down (pelvis under 0.35 m) and sleeps within 3.5 s",
        measured: format!("pelvis {pelvis:.2} m, asleep after {slept:?} s"),
        pass: pelvis < 0.35 && slept.is_some_and(|s| s < 3.5),
    });

    // Driven in the air, a body falls as gravity says: the drives shape it
    // but never hold it up.
    let mut world = World::new();
    let high: Vec<Frame> = places
        .iter()
        .map(|p| Frame::new(p.translation + Vec3::Z * 10.0, p.rotation))
        .collect();
    let id = world.spawn(template.clone(), &high, Vec3::ZERO, false);
    run(&mut world, (id, 0.5), Some((&high, held)));
    let now = world.places(id).expect("the ragdoll is there");
    let fell = high[0].translation.z - now[0].translation.z;
    let free = 0.5 * 9.81 * 0.5 * 0.5;
    probes.push(Probe {
        name: "unheld",
        expect: "held at full strength to a pose 10 m up, a body still falls freely: 0.5 s drops it ½gt² (1.23 m) within 5%",
        measured: format!("fell {fell:.3} m"),
        pass: (fell - free).abs() < free * 0.05,
    });

    // A droid fitted without a plan is few, stiff parts; it falls over
    // whole and rests.
    let (nodes, skin) = droid();
    let skeleton = Skeleton {
        nodes: &nodes,
        skin: &skin,
    };
    let fitted = Template::auto(&skeleton, Material::Metal);
    let measured = match &fitted {
        Ok(droid) => {
            let template = Arc::new(droid.clone());
            let places = rest_places(&template);
            let mut world = floored();
            let id = world.spawn(template.clone(), &places, Vec3::new(1.5, 0.0, 0.0), false);
            run(&mut world, (id, 5.0), None);
            let now = world.places(id).expect("the ragdoll is there");
            // How far each leg bent from the body against how it stood.
            let bent = template
                .parts
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, p)| {
                    let parent = p.parent.unwrap_or(0);
                    let was = places[parent].rotation.inverse() * places[i].rotation;
                    let is = now[parent].rotation.inverse() * now[i].rotation;
                    was.angle_between(is).to_degrees()
                })
                .fold(0.0, f32::max);
            Some((template.parts.len(), droid.mass, bent, world.resting(id)))
        }
        Err(_) => None,
    };
    probes.push(Probe {
        name: "droid",
        expect: "a metal walker fitted from its mesh alone has its 5 pieces as parts, weighs more than a person, and toppled, no joint bends past 20° and it rests within 5 s",
        measured: measured.map_or("not fitted".into(), |(parts, mass, bent, rests)| {
            format!("{parts} parts, {mass:.0} kg, worst bend {bent:.1}°, rests {rests}")
        }),
        pass: measured.is_some_and(|(parts, mass, bent, rests)| {
            parts == 5 && mass > humanoid::MASS && bent < 20.0 && rests
        }),
    });

    // A metal turret is fitted without a plan. Its light plates hang on its
    // heavy base, and its root node carries no mesh. Pushed over, it falls
    // and sleeps.
    let (nodes, skin) = turret();
    let skeleton = Skeleton {
        nodes: &nodes,
        skin: &skin,
    };
    let measured = Template::auto(&skeleton, Material::Metal)
        .ok()
        .map(|turret| {
            let template = Arc::new(turret);
            let places = rest_places(&template);
            let mut world = floored();
            let id = world.spawn(template.clone(), &places, Vec3::new(1.5, 0.0, 0.0), false);
            let mut slept = None;
            for step in 0..(8.0 / STEP) as u32 {
                world.step(STEP);
                if slept.is_none() && world.resting(id) {
                    slept = Some(step as f32 * STEP);
                }
            }
            (template.parts.len(), slept)
        });
    probes.push(Probe {
        name: "turret",
        expect: "a metal turret fitted from its mesh alone has its 7 pieces as parts, and pushed over, it sleeps within 8 s",
        measured: measured.map_or("not fitted".into(), |(parts, slept)| {
            format!("{parts} parts, asleep after {slept:?} s")
        }),
        pass: measured.is_some_and(|(parts, slept)| parts == 7 && slept.is_some_and(|s| s < 8.0)),
    });

    probes.push(walled(&template, &places));
    probes.push(beside_corpse(&template, &places));
    probes.push(sever(&template, &places));
    probes.push(cut());
    probes
}

/// Where the probes' wall stands (x, metres): a single face, as a KOTOR
/// room's drawn wall is, through the standing figure's right forearm.
const WALL_X: f32 = 0.4;

/// A figure dying with its right arm through a wall (a face from the
/// floor up, facing the figure), as a creature killed against one is: it
/// falls on its side of the wall, no part of it left behind the wall. Let
/// go at once, and held for a second to its pose (the arm through the
/// wall) before it goes limp, as a death animation holds it.
fn walled(template: &Arc<Template>, places: &[Frame]) -> Probe {
    let behind = |held: bool| -> Vec<&'static str> {
        let mut world = floored();
        let s = 20.0;
        let [a, b, c, d] =
            [[-s, 0.0], [s, 0.0], [s, 3.0], [-s, 3.0]].map(|[y, z]| Vec3::new(WALL_X, y, z));
        world.add_faces([(1, [a, c, b]), (1, [a, d, c])]);
        let id = world.spawn(template.clone(), places, Vec3::ZERO, false);
        if held {
            let full = Muscles {
                hold: 1.0,
                tone: 1.0,
            };
            run(&mut world, (id, 1.0), Some((places, full)));
        }
        run(&mut world, (id, 3.0), None);
        let now = world.places(id).expect("the ragdoll is there");
        template
            .parts
            .iter()
            .zip(&now)
            .filter(|(part, place)| place.point(part.solid.centre()).x > WALL_X)
            .map(|(part, _)| part.name)
            .collect()
    };
    let (limp, held) = (behind(false), behind(true));
    Probe {
        name: "wall",
        expect: "a figure whose arm stands through a wall falls on the wall's near side, let go at once or held to its pose for a second, no part's middle behind it",
        measured: format!("behind the wall: let go {limp:?}, held {held:?}"),
        pass: limp.is_empty() && held.is_empty(),
    }
}

/// The game's muscle curve (`game::ragdolls::muscles`) `since_ms` into a
/// death clip `length_ms` long.
fn dying(since_ms: f64, length_ms: f64) -> Muscles {
    let t = (since_ms / length_ms) as f32;
    let ease = |from: f32, to: f32| {
        let x = ((t - from) / (to - from)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let tone = if t < 1.0 {
        1.0 - 0.7 * ease(0.45, 1.0)
    } else {
        0.3 * (1.0 - ((since_ms - length_ms) / 700.0) as f32).max(0.0)
    };
    Muscles {
        hold: 0.85 * (1.0 - ease(0.0, 0.6)),
        tone,
    }
}

/// A figure dies standing 0.3 m beside a corpse asleep on the floor,
/// touching it. It is pushed at 0.4 m/s and driven toward its standing pose
/// by the game's muscle curve over a death clip of 1 s. A body that falls
/// asleep wakes no other body, so the dying figure must not be put to sleep
/// with the corpse while its muscles still hold it up.
fn beside_corpse(template: &Arc<Template>, places: &[Frame]) -> Probe {
    let mut world = floored();
    let lying: Vec<Frame> = places
        .iter()
        .map(|place| {
            let turn = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
            Frame::new(
                turn * place.translation + Vec3::new(0.0, 1.0, 0.25),
                turn * place.rotation,
            )
        })
        .collect();
    let corpse = world.spawn(template.clone(), &lying, Vec3::ZERO, false);
    for _ in 0..(10.0 / STEP) as u32 {
        if world.resting(corpse) {
            break;
        }
        world.step(STEP);
    }
    let standing: Vec<Frame> = places
        .iter()
        .map(|place| Frame::new(place.translation + Vec3::new(0.3, 1.0, 0.2), place.rotation))
        .collect();
    let id = world.spawn(
        template.clone(),
        &standing,
        Vec3::new(-0.4, 0.0, 0.0),
        false,
    );
    let mut slept = None;
    for step in 0..(12.0 / STEP) as u32 {
        let seconds = f64::from(step) * f64::from(STEP);
        world.drive(id, &standing, dying(seconds * 1000.0, 1000.0), seconds);
        world.step(STEP);
        if slept.is_none() && world.resting(id) {
            slept = Some(step as f32 * STEP);
        }
    }
    let now = world.places(id).expect("the ragdoll is there");
    let head = now[part(template, "head")].translation.z;
    Probe {
        name: "beside corpse",
        expect: "a figure dying beside a sleeping corpse, its muscles letting go over a 1 s death clip, lies down by 12 s (head under 0.8 m)",
        measured: format!("head {head:.2} m, asleep after {slept:?} s"),
        pass: head < 0.8,
    }
}

/// Arms cut off standing bodies, thrown a little differently each time,
/// leave their shoulders whole (their elbows hold) while the bodies fall
/// and rest; a part already cut, one hanging from it, or the root, is not
/// cut.
fn sever(template: &Arc<Template>, places: &[Frame]) -> Probe {
    let (chest, arm, forearm) = (
        part(template, "chest"),
        part(template, "upper arm left"),
        part(template, "forearm left"),
    );
    // Where the shoulder is on the chest, and the elbow on the arm.
    let shoulder = places[chest].inverse().point(places[arm].translation);
    let elbow = places[arm].inverse().point(places[forearm].translation);
    let (mut refused, mut gap, mut held, mut slowest) = (true, f32::MAX, 0.0f32, Some(0.0f32));
    for throw in 0..THROWS {
        let mut world = floored();
        let id = world.spawn(template.clone(), places, Vec3::ZERO, false);
        let side = throw as f32 * 0.02;
        let blow = (
            Vec3::new(-6.0 - side, side, 6.0),
            places[arm].translation,
            Vec3::ZERO,
        );
        let cut = world.sever(id, arm, blow, 0.05);
        refused &= cut
            && !(world.sever(id, arm, blow, 0.0)
                || world.sever(id, forearm, blow, 0.0)
                || world.sever(id, 0, blow, 0.0));
        let mut slept = None;
        for step in 0..(REST_WITHIN / STEP) as u32 {
            world.step(STEP);
            if slept.is_none() && world.resting(id) {
                slept = Some(step as f32 * STEP);
            }
        }
        let now = world.places(id).expect("the ragdoll is there");
        gap = gap.min(now[chest].point(shoulder).distance(now[arm].translation));
        held = held.max(now[arm].point(elbow).distance(now[forearm].translation));
        slowest = slowest.zip(slept).map(|(a, b)| a.max(b));
    }
    Probe {
        name: "sever",
        expect: "arms cut off and thrown 12 ways each land over 0.5 m from the shoulder with the elbow within 3 cm, every body rests within 5 s, and a second cut (of it, its forearm or the root) is refused",
        measured: format!(
            "refused {refused}, nearest shoulder gap {gap:.2} m, worst elbow {held:.3} m, slowest rest {slowest:?} s"
        ),
        pass: refused && gap > 0.5 && held < 0.03 && slowest.is_some(),
    }
}

/// How many throws `sever` tries, and how soon each body must rest (s).
const THROWS: u32 = 12;
const REST_WITHIN: f32 = 5.0;

/// A tube along +X skinned to two bones (slot 0 the body's below x 0.5,
/// slot 1 the limb's above, blended at 0.5), open at both ends: `RINGS`
/// rings of `SIDES` corners, its triangles facing out.
fn tube() -> SkinnedMesh {
    const RINGS: u32 = 5;
    const SIDES: u32 = 8;
    let mut mesh = SkinnedMesh::default();
    for ring in 0..RINGS {
        let x = ring as f32 / (RINGS - 1) as f32;
        let limb = ((x - 0.25) * 2.0).clamp(0.0, 1.0);
        for side in 0..SIDES {
            let turn = side as f32 / SIDES as f32 * std::f32::consts::TAU;
            let (y, z) = (turn.cos() * 0.1, turn.sin() * 0.1);
            mesh.positions.push([x, y, z]);
            mesh.normals.push([0.0, y * 10.0, z * 10.0]);
            mesh.uvs.push([0.0, 0.0]);
            mesh.joints.push([0, 1, 0, 0]);
            mesh.weights.push([1.0 - limb, limb, 0.0, 0.0]);
        }
    }
    for ring in 0..RINGS - 1 {
        for side in 0..SIDES {
            let corner = |r: u32, s: u32| r * SIDES + s % SIDES;
            let (a, b) = (corner(ring, side), corner(ring, side + 1));
            let (c, d) = (corner(ring + 1, side), corner(ring + 1, side + 1));
            mesh.indices.extend([a, b, d, a, d, c]);
        }
    }
    mesh
}

/// Two squares meeting at one corner (the origin), on one bone: two holes
/// pinched through a vertex, as a cut can leave them.
fn bow_tie() -> SkinnedMesh {
    let corners: [[f32; 3]; 7] = [
        [0.0, 0.0, 0.0],
        [0.1, 0.0, 0.0],
        [0.1, 0.1, 0.0],
        [0.0, 0.1, 0.0],
        [-0.1, 0.0, 0.0],
        [-0.1, -0.1, 0.0],
        [0.0, -0.1, 0.0],
    ];
    SkinnedMesh {
        positions: corners.to_vec(),
        normals: vec![[0.0, 0.0, 1.0]; 7],
        uvs: vec![[0.0, 0.0]; 7],
        joints: vec![[0; 4]; 7],
        weights: vec![[1.0, 0.0, 0.0, 0.0]; 7],
        indices: vec![0, 1, 2, 0, 2, 3, 0, 4, 5, 0, 5, 6],
    }
}

/// A skinned tube cut at its joint: every triangle on one side, neither
/// side skinned to the other's bone (what stretches a cut), and the cut
/// closed on each side by one cap of the ring's 8 triangles facing out of
/// it; the tube's own open ends, away from the joint, left open.
fn cut() -> Probe {
    let whole = tube();
    let limb = |slot: u16| slot == 1;
    let (stays, goes) = whole.cut(&Cut {
        limb: &limb,
        stump: 0,
        root: 1,
    });
    let triangles = |mesh: &SkinnedMesh| mesh.indices.len() / 3;
    let on = |mesh: &SkinnedMesh, slot: u16| {
        mesh.joints
            .iter()
            .zip(&mesh.weights)
            .any(|(j, w)| (0..4).any(|i| j[i] == slot && w[i] > 0.0))
    };
    let wound = |slot| Wound {
        at: [0.5, 0.0, 0.0],
        within: 0.2,
        slot,
    };
    let (stump, end) = (stays.caps(&wound(0)), goes.caps(&wound(1)));
    // Which way a cap faces, along the tube.
    let facing = |cap: &SkinnedMesh| cap.normals.first().map_or(0.0, |n| n[0]);
    let (kept, gone) = (triangles(&stays), triangles(&goes));
    let crossed = on(&stays, 1) || on(&goes, 0);
    let (stump_caps, end_caps) = (triangles(&stump), triangles(&end));
    let (stump_faces, end_faces) = (facing(&stump), facing(&end));
    // Where the limb meets the body, had no node above the cut a slot.
    let blended = whole.blended(&limb);
    // Two holes through one corner: both capped.
    let pinched = triangles(&bow_tie().caps(&Wound {
        at: [0.0, 0.0, 0.0],
        within: 0.2,
        slot: 0,
    }));
    Probe {
        name: "cut",
        expect: "a two-bone tube of 64 triangles cut at its joint: 32 stay and 32 go, neither side skinned to the other's bone, each side's cut closed by 8 triangles facing out of it (+X on the stump, −X on the limb), the limb's skin blends into the body's bone, and two holes pinched through one corner are both capped (8 triangles)",
        measured: format!(
            "{kept} stay, {gone} go, crossed {crossed}, caps {stump_caps} facing {stump_faces:.2} and {end_caps} facing {end_faces:.2}, blends into {blended:?}, pinched caps {pinched}"
        ),
        pass: kept + gone == triangles(&whole)
            && kept == 32
            && !crossed
            && stump_caps == 8
            && end_caps == 8
            && stump_faces > 0.9
            && end_faces < -0.9
            && blended == Some(0)
            && pinched == 8,
    }
}
