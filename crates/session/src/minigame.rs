//! An area's minigame for the sim (`sim::minigame::Rules`): the ARE's
//! `MiniGame` struct (`asset_odyssey::minigame`) with each track's path,
//! read from its model (the `modelhook` node over the `track` clip, placed
//! where the LYT puts the model), and each obstacle where the LYT puts it,
//! as wide as its model's `hitbump` node; each follower's models' clips,
//! and where each gun bank's bullets leave (its gun model's `bullethook0`
//! on the follower's `gunbank<id>`).

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::minigame::{FollowerIr, GunBankIr, MiniGameIr};
use asset_odyssey::model::ModelIr;
use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::mdl::Model;
use sim::minigame::{Bank, Follower, Hooks, Kind, Obstacle, Path, Rules};

/// The node a track's follower hangs on, and the clip that moves it.
const HOOK: &str = "modelhook";
const TRACK_CLIP: &str = "track";
/// An obstacle's half size across the ground when its model has no
/// `hitbump` mesh (metres) [verify].
const OBSTACLE_EXTENT: f32 = 2.0;

fn model(source: &dyn Source, name: &str) -> Option<Model> {
    let mdl = source.read(name, ResType::MDL)?;
    let mdx = source.read(name, ResType::MDX).unwrap_or(Vec::new());
    Model::parse(&mdl, &mdx).ok()
}

fn ms(seconds: f32) -> u32 {
    (seconds.max(0.0) * 1000.0).round() as u32
}

/// Model `name`'s own name, as its MDL spells it (`211TEL_MGM05`); the
/// resref without one.
fn spelled(model: Option<&Model>, name: &str) -> String {
    model
        .map(|model| model.name.clone())
        .filter(|spelled| !spelled.is_empty())
        .unwrap_or_else(|| name.to_string())
}

/// Track `name`'s path, placed at `place` (the LYT's), and the track
/// model's name as its MDL spells it.
fn path(source: &dyn Source, name: &str, place: [f32; 3]) -> (Path, String) {
    let Some(model) = model(source, name) else {
        let path = Path {
            origin: place,
            rest: [0.0, 0.0, 0.0, 1.0],
            ..Path::default()
        };
        return (path, name.to_string());
    };
    let ir = ModelIr::from_model(&model);
    let hook = ir
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case(HOOK));
    let rest = hook.map_or([0.0; 3], |node| {
        [node.world[0][3], node.world[1][3], node.world[2][3]]
    });
    let turn = hook.map_or([0.0, 0.0, 0.0, 1.0], |node| node.rotation);
    let clip = model
        .animations
        .iter()
        .find(|clip| clip.name.eq_ignore_ascii_case(TRACK_CLIP))
        .map(|clip| AnimationIr::from_animation(clip, &model.name));
    let track = clip.as_ref().and_then(|clip| {
        clip.tracks
            .iter()
            .find(|track| track.node.eq_ignore_ascii_case(HOOK))
    });
    let scale = if ir.animation_scale > 0.0 {
        ir.animation_scale
    } else {
        1.0
    };
    // The hook's position keys' bezier handles (in, out: after the value),
    // scaled as the keys are.
    let handles = model
        .animations
        .iter()
        .find(|clip| clip.name.eq_ignore_ascii_case(TRACK_CLIP))
        .and_then(|clip| find_node(&clip.root, HOOK))
        .and_then(|node| {
            node.controllers
                .iter()
                .find(|c| c.kind == 8 && c.columns == 3 && c.bezier)
        })
        .map_or(Vec::new(), |controller| {
            (0..controller.times.len())
                .filter_map(|row| {
                    let at = row * 9;
                    let v = controller.values.get(at..at + 9)?;
                    Some((
                        [v[3] * scale, v[4] * scale, v[5] * scale],
                        [v[6] * scale, v[7] * scale, v[8] * scale],
                    ))
                })
                .collect()
        });
    // A hook carried by a node above it that the clip moves (505OND's
    // fighters: `path_dummy02` keyed, the hook under it keyed only in its
    // turn): its keys are the chain's, posed down to the hook as the model
    // draws it (`anim::posed`), at every key time of the chain.
    if let (Some(clip), Some(at)) = (clip.as_ref(), hook_index(&ir))
        && let Some(path) = carried(
            &ir,
            clip,
            (at, scale),
            [0, 1, 2].map(|i| place[i] + rest[i]),
        )
    {
        return (path, spelled(Some(&model), name));
    }
    let path = Path {
        origin: [0, 1, 2].map(|i| place[i] + rest[i]),
        handles,
        rest: turn,
        positions: track.map_or_else(Vec::new, |track| {
            track
                .positions
                .iter()
                .map(|(time, offset)| (*time, offset.map(|v| v * scale)))
                .collect()
        }),
        orientations: track.map_or_else(Vec::new, |track| track.orientations.clone()),
        length: clip.as_ref().map_or(0.0, |clip| clip.length),
    };
    (path, spelled(Some(&model), name))
}

/// The track model's hook node.
fn hook_index(ir: &ModelIr) -> Option<usize> {
    ir.nodes
        .iter()
        .position(|node| node.name.eq_ignore_ascii_case(HOOK))
}

/// The path of hook `hook` when `clip` moves a node above it: each key time
/// of the chain from the root to the hook, the hook's place posed there (an
/// offset from its rest place, `origin`) and its turn; `None` when no node
/// above the hook is keyed (the hook's own keys are its path).
fn carried(
    ir: &ModelIr,
    clip: &AnimationIr,
    (hook, scale): (usize, f32),
    origin: [f32; 3],
) -> Option<Path> {
    let bound = clip.bind(ir);
    let mut chain = Vec::new();
    let mut at = ir.nodes.get(hook)?.parent;
    while let Some(node) = at {
        chain.push(node);
        at = ir.nodes[node].parent;
    }
    let keyed = |node: usize| {
        bound
            .get(node)
            .copied()
            .flatten()
            .and_then(|t| clip.tracks.get(t))
    };
    if !chain.iter().any(|&node| {
        keyed(node).is_some_and(|t| !t.positions.is_empty() || !t.orientations.is_empty())
    }) {
        return None;
    }
    chain.push(hook);
    let mut times: Vec<f32> = chain
        .iter()
        .filter_map(|&node| keyed(node))
        .flat_map(|t| {
            t.positions
                .iter()
                .map(|k| k.0)
                .chain(t.orientations.iter().map(|k| k.0))
        })
        .collect();
    times.sort_by(f32::total_cmp);
    times.dedup();
    let rest = ir.nodes[hook].world;
    let posed = |t: f32| asset_odyssey::anim::posed(ir, (clip, &bound), scale, hook, t);
    Some(Path {
        origin,
        rest: turn_of(&rest),
        positions: times
            .iter()
            .map(|&t| {
                let m = posed(t);
                (t, [0, 1, 2].map(|i| m[i][3] - rest[i][3]))
            })
            .collect(),
        handles: Vec::new(),
        orientations: times.iter().map(|&t| (t, turn_of(&posed(t)))).collect(),
        length: clip.length,
    })
}

/// A transform's turn as a quaternion (x, y, z, w), from its rotation
/// (row-major, as `NodeIr::world`).
fn turn_of(m: &[[f32; 4]; 4]) -> [f32; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        ]
    };
    let n = q
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    q.map(|v| v / n)
}

/// The node named `name` under `node`.
fn find_node<'a>(
    node: &'a fmt_odyssey::mdl::Node,
    name: &str,
) -> Option<&'a fmt_odyssey::mdl::Node> {
    if node.name.eq_ignore_ascii_case(name) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, name))
}

/// Obstacle `name`'s hits' box, placed at `place`: its `hitbump` node's
/// mesh's middle and half sizes; and its model's name as the MDL spells it.
fn bump(source: &dyn Source, name: &str, place: [f32; 3]) -> ([f32; 3], [f32; 3], String) {
    let read = model(source, name);
    let spelled = spelled(read.as_ref(), name);
    let found = read.and_then(|model| {
        let node = model
            .nodes()
            .into_iter()
            .find(|node| node.name.eq_ignore_ascii_case("hitbump"))?;
        let bounds = node.mesh.as_ref()?.bounds;
        let centre = [0, 1, 2].map(|i| (bounds[i] + bounds[i + 3]) / 2.0 + node.position[i]);
        let extent = [0, 1, 2].map(|i| (bounds[i + 3] - bounds[i]) / 2.0);
        (extent[0] > 0.0 && extent[1] > 0.0).then_some((centre, extent))
    });
    match found {
        Some((centre, extent)) => ([0, 1, 2].map(|i| place[i] + centre[i]), extent, spelled),
        None => (place, [OBSTACLE_EXTENT; 3], spelled),
    }
}

/// A node's rest transform in its model's space (row-major, as `NodeIr`
/// keeps it), applied to `point`.
fn apply(world: &[[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| {
        world[row][0] * point[0]
            + world[row][1] * point[1]
            + world[row][2] * point[2]
            + world[row][3]
    })
}

/// Where gun bank `id`'s bullets leave and which way, in its follower's
/// space: the gun model's `bullethook0` hung on the first of `models` with
/// a `gunbank<id>` node, along the node's +y; that node alone without the
/// hook; the middle and +y without the node.
fn muzzle(source: &dyn Source, models: &[ModelIr], id: u32, gun: &str) -> ([f32; 3], [f32; 3]) {
    let name = format!("gunbank{id}");
    let Some(bank) = models.iter().find_map(|ir| {
        ir.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(&name))
    }) else {
        return ([0.0; 3], [0.0, 1.0, 0.0]);
    };
    let along = [0, 1, 2].map(|row| bank.world[row][1]);
    let length = (along[0] * along[0] + along[1] * along[1] + along[2] * along[2]).sqrt();
    let way = if length > 1e-6 {
        along.map(|v| v / length)
    } else {
        [0.0, 1.0, 0.0]
    };
    let hook = model(source, gun).and_then(|gun| {
        let ir = ModelIr::from_model(&gun);
        let node = ir
            .nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case("bullethook0"))?;
        Some([node.world[0][3], node.world[1][3], node.world[2][3]])
    });
    (apply(&bank.world, hook.unwrap_or([0.0; 3])), way)
}

fn bank(ir: &GunBankIr, (muzzle, way): ([f32; 3], [f32; 3])) -> Bank {
    Bank {
        id: ir.id,
        gun_model: ir.gun_model.clone(),
        bullet_model: ir.bullet_model.clone(),
        damage: ir.damage,
        lifespan_ms: ms(ir.lifespan),
        rate_ms: ms(ir.rate_of_fire),
        speed: ir.speed,
        target: (ir.target_type & 3) as u8,
        fire_sound: ir.fire_sound.clone(),
        hit_sound: ir.collision_sound.clone(),
        inaccuracy: ir.inaccuracy,
        sensing: ir.sensing_radius,
        spread: [ir.horiz_spread, ir.vert_spread],
        muzzle,
        way,
    }
}

/// Where the LYT places `name`, else the origin.
fn place(places: &[(String, [f32; 3])], name: &str) -> [f32; 3] {
    places
        .iter()
        .find(|(model, _)| model.eq_ignore_ascii_case(name))
        .map_or([0.0; 3], |(_, at)| *at)
}

fn follower(source: &dyn Source, ir: &FollowerIr, game: &MiniGameIr) -> Follower {
    let read: Vec<Option<Model>> = ir
        .models
        .iter()
        .map(|(name, _)| model(source, name))
        .collect();
    let clips = read
        .iter()
        .map(|model| {
            model.as_ref().map_or(Vec::new(), |model| {
                model
                    .animations
                    .iter()
                    .map(|clip| clip.name.to_ascii_lowercase())
                    .collect()
            })
        })
        .collect();
    let irs: Vec<ModelIr> = read.iter().flatten().map(ModelIr::from_model).collect();
    let (path, name) = path(source, &ir.track, place(&game.tracks, &ir.track));
    Follower {
        name,
        path,
        loops: ir.num_loops,
        radius: ir.sphere_radius,
        invince_ms: ms(ir.invince_period),
        hp: ir.hit_points,
        max_hp: ir.max_hps,
        bump_damage: ir.bump_damage,
        models: ir.models.clone(),
        clips,
        hooks: Hooks::from_labels(&ir.scripts),
        camera: ir.camera.clone(),
        camera_rotate: ir.camera_rotate,
        min_speed: ir.min_speed,
        max_speed: ir.max_speed,
        start_offset: ir.start_offset,
        tunnel_infinite: ir.tunnel_infinite.map(|v| v != 0.0),
        tunnel_pos: ir.tunnel_pos,
        tunnel_neg: ir.tunnel_neg,
        target_offset: ir.target_offset,
        trigger: ir.trigger,
        banks: ir
            .gun_banks
            .iter()
            .map(|gun| bank(gun, muzzle(source, &irs, gun.id, &gun.gun_model)))
            .collect(),
        death_sound: ir.death_sound.clone(),
        engine_sound: ir.engine_sound.clone(),
    }
}

/// The sim's rules for the minigame `ir` describes, its enemies scaled by
/// `per_player_hp` for each gunner past the first (the module's encounter
/// file's).
pub fn rules(source: &dyn Source, ir: &MiniGameIr, per_player_hp: f32) -> Rules {
    let axis = |axis: u32, default: u8| match axis {
        1..=3 => (axis - 1) as u8,
        _ => default,
    };
    let (mouse, flip) = match ir.mouse {
        Some(([x, y], flip)) => ([axis(x, 2), axis(y, 0)], flip),
        None => ([2, 0], [false; 2]),
    };
    Rules {
        kind: if ir.kind == 2 {
            Kind::Turret
        } else {
            Kind::Swoop
        },
        movement_per_sec: ir.movement_per_sec,
        lateral_accel: ir.lateral_accel,
        use_inertia: ir.use_inertia,
        do_bumping: ir.do_bumping,
        clip: [ir.near_clip, ir.far_clip],
        view_angle: ir.view_angle,
        music: ir.music.clone(),
        mouse,
        flip,
        player: follower(source, &ir.player, ir),
        enemies: ir
            .enemies
            .iter()
            .map(|enemy| follower(source, enemy, ir))
            .collect(),
        obstacles: ir
            .obstacles
            .iter()
            .map(|obstacle| {
                let position = place(&ir.obstacle_places, &obstacle.name);
                let (centre, extent, name) = bump(source, &obstacle.name, position);
                Obstacle {
                    model: obstacle.name.clone(),
                    name,
                    position,
                    centre,
                    extent,
                    hooks: Hooks::from_labels(&obstacle.scripts),
                }
            })
            .collect(),
        per_player_hp,
        tsl: ir.tsl,
    }
}

/// A fighter's hook carried by a keyed node above it (`carried`), on a
/// track model made here: `dummy` keyed 10 m along x over two seconds,
/// turned a quarter round at its end, the hook 1 m up from it and keyed
/// in nothing; the path follows the dummy (the hook's place 5 m along at
/// a second, 10 m at the end), held at the clip's length; a hook keyed
/// itself keeps its own keys.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    use asset_odyssey::anim::NodeTrack;
    use asset_odyssey::model::{IDENTITY, NodeIr};
    let node = |name: &str, parent: Option<usize>, translation: [f32; 3]| NodeIr {
        name: name.into(),
        parent,
        translation,
        rotation: [0.0, 0.0, 0.0, 1.0],
        world: IDENTITY,
        animated: true,
    };
    let mut hook = node(HOOK, Some(1), [0.0, 0.0, 1.0]);
    hook.world[2][3] = 1.0;
    let ir = ModelIr {
        name: "track".into(),
        supermodel: None,
        nodes: vec![
            node("track", None, [0.0; 3]),
            node("dummy", Some(0), [0.0; 3]),
            hook,
        ],
        meshes: Vec::new(),
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
    };
    let quarter = [
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    let clip = |keyed: &str| AnimationIr {
        name: TRACK_CLIP.into(),
        length: 2.0,
        transition: 0.0,
        root: None,
        tracks: vec![NodeTrack {
            node: keyed.into(),
            positions: vec![(0.0, [0.0; 3]), (2.0, [10.0, 0.0, 0.0])],
            orientations: vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (2.0, quarter)],
        }],
        settings: Vec::new(),
        events: Vec::new(),
    };
    let origin = [100.0, 0.0, 1.0];
    let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3);
    let carried_path = carried(&ir, &clip("dummy"), (2, 1.0), origin);
    let own = carried(&ir, &clip(HOOK), (2, 1.0), origin);
    vec![
        (
            "track_hook_carried",
            match &carried_path {
                Some(path) => {
                    let (mid, end, after) = (path.at(1.0), path.at(2.0), path.at(5.0));
                    check(
                        close(mid, [105.0, 0.0, 1.0])
                            && close(end, [110.0, 0.0, 1.0])
                            && close(after, end),
                        format!("at 1 s {mid:?}, at 2 s {end:?}, past the end {after:?}"),
                    )
                }
                None => Err("the dummy's keys were not taken".into()),
            },
        ),
        (
            "track_hook_own_keys",
            check(own.is_none(), format!("{:?}", own.map(|p| p.positions))),
        ),
    ]
}
