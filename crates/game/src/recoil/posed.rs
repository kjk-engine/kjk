//! The firing motion measured on a posed KOTOR body. JA's ready pose for a
//! rifle (aimed and from the hip) and for a pair of pistols is retargeted
//! to Carth's body (`p_carthbb`) with K1's guns held up
//! (`asset_ja::rig::Aim`) and pushed by a shot (`push`). The body's turns
//! (`turns`) are added after the arms are posed, as `cast::recoil` adds
//! them. These checks need JA and KOTOR installed.

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

/// The body, its guns and JA's skeleton.
struct Scene {
    ja: Ja,
    body: BodyRig,
    rifle: GunShape,
    pistol: GunShape,
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
        Ok(Scene {
            ja,
            body,
            rifle,
            pistol,
        })
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
fn aim(side: Side, shape: &GunShape, target: Vec3, push: Push, support: Option<f32>) -> Aim {
    Aim {
        side,
        target,
        muzzle: shape.muzzle,
        barrel: shape.barrel,
        up: shape.up,
        support: support.map(|share| shape.foregrip(share)),
        forward: 0.0,
        kick: (push.up_deg.to_radians(), push.back_m),
        shoulder: push.shoulder_deg.to_radians(),
        weight: 1.0,
        hold: None,
        stow: None,
    }
}

/// One moment of a posed body firing: each node's place.
type Moment = (f64, Vec<(Vec3, Quat)>);

/// A body standing ready in `pose` with `wield`'s guns, whose right hand
/// fires once at 0, posed every `STEP_MS` over `SPAN_MS`. Also its right
/// axis and its forward axis (the model's space).
fn fired(
    scene: &Scene,
    wield: Wield,
    pose: &[asset_ja::g2::Local],
) -> Result<(Vec<Moment>, Vec3, Vec3), String> {
    let (shape, blaster) = match wield {
        Wield::Rifle => (&scene.rifle, Blaster::Rifle),
        _ => (&scene.pistol, Blaster::Pistol),
    };
    let support = (wield == Wield::Rifle).then_some(RIFLE_SUPPORT);
    let retargeter = &scene.body.retargeter;
    let mut off = OffHand::default();
    let still = retargeter.pose(pose, Vec3::ZERO, (&[], &[]), (&mut off, 0.0), 0.0);
    let still = scene.globals(&still, &[]);
    let [right, left] = ["rbicep_g", "lbicep_g"].map(|name| scene.node(name));
    let right = (still[right?].0 - still[left?].0) * Vec3::new(1.0, 1.0, 0.0);
    let right = right
        .try_normalize()
        .ok_or("the shoulders stand one above the other")?;
    let forward = Vec3::Z.cross(right);
    let weapons = [scene.node("rhand")?, scene.node("lhand")?];
    let targets = weapons.map(|node| {
        let (at, turn) = still[node];
        at + turn * shape.muzzle + forward * AHEAD
    });
    let hands: &[(usize, Side)] = match wield {
        Wield::Pair => &[(0, Side::Right), (1, Side::Left)],
        _ => &[(0, Side::Right)],
    };
    let mut moments = Vec::new();
    let mut now = SPAN_MS.0;
    while now <= SPAN_MS.1 {
        let firing = Firing {
            wield: Some(wield),
            blasters: hands.iter().fold([None; 2], |mut guns, &(hand, _)| {
                guns[hand] = Some(blaster);
                guns
            }),
            shots: Shots {
                last: [Some(0.0), None],
                before: [None; 2],
            },
            streak: Streak::default(),
            looked_through: false,
            alive: true,
        };
        let aims: Vec<Aim> = hands
            .iter()
            .map(|&(hand, side)| {
                let push = push(&firing, hand, now);
                aim(side, shape, targets[hand], push, support)
            })
            .collect();
        let posed = retargeter.pose(pose, Vec3::ZERO, (&[], &aims), (&mut off, now as f32), 0.0);
        let after: Vec<(usize, Quat)> = turns(&firing, now)
            .iter()
            .filter_map(|turn| Some((scene.node(turn.node).ok()?, rotation(turn, right, Vec3::Z))))
            .collect();
        moments.push((now, scene.globals(&posed, &after)));
        now += STEP_MS;
    }
    Ok((moments, right, forward))
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
        let (moments, _, forward) = fired(scene, Wield::Rifle, &pose)?;
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
        let how = if aimed { "aimed" } else { "from the hip" };
        let what = format!(
            "{how}: the support hand is {rest:.3} m from its grip before the shot and at most {support:.4} m further during it; the shoulder goes {back:.4} m back and {up:.4} m up; the muzzle {along:.4} m back along the barrel and {lift:.4} m up"
        );
        failed |= support > SUPPORT_MOST || back < 0.003 || up > back / 2.0 || along <= lift.abs();
        seen.push(what);
    }
    check(!failed, seen.join("; "))
}

/// A pair aimed, the right gun firing: the left gun moves no more than
/// `STILL_MOST`, and the right one at least 2 cm.
fn pair(scene: &Scene) -> Result<String, String> {
    let weapons = [scene.node("rhand")?, scene.node("lhand")?];
    let pose = scene.ja_pose(Wield::Pair, true)?;
    let (moments, ..) = fired(scene, Wield::Pair, &pose)?;
    let [right, left] =
        weapons.map(|node| drift(moments.iter().map(|(_, globals)| globals[node].0)));
    check(
        left <= STILL_MOST && right >= 0.02,
        format!("the right gun moves {right:.4} m, the left {left:.4} m"),
    )
}

pub(super) fn checks() -> sim::probe::Checks {
    let names = ["rifle_into_shoulder", "pair_other_gun_still"];
    match Scene::load() {
        Ok(scene) => vec![(names[0], rifle(&scene)), (names[1], pair(&scene))],
        Err(why) => names
            .map(|name| (name, Ok(format!("skipped: {why}"))))
            .into(),
    }
}
