//! An area's minigame as its ARE describes it (the `MiniGame` struct: the
//! swoop race, `Type` 1, and the turret, `Type` 2), with the places its LYT
//! gives the tracks and obstacles (`trackcount`, `obstaclecount`). Read for
//! the facts reone's reader of the ARE takes; the meaning of each is
//! the sim's (`sim::minigame`, docs/MINIGAMES.md).

use fmt_odyssey::gff::GffStruct;
use fmt_odyssey::lyt::Layout;

#[derive(Debug, Clone, PartialEq)]
pub struct MiniGameIr {
    /// 1: the player moves along its track (a swoop); 2: it turns on it (a
    /// turret).
    pub kind: u32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub view_angle: f32,
    pub use_inertia: bool,
    pub do_bumping: bool,
    pub movement_per_sec: f32,
    pub lateral_accel: f32,
    pub music: String,
    /// (`AxisX`, `AxisY`, `FlipAxisX`, `FlipAxisY`): which offset axis (1
    /// x, 2 y, 3 z) the mouse's moves turn; none where the ARE has no
    /// `Mouse`.
    pub mouse: Option<([u32; 2], [bool; 2])>,
    pub player: FollowerIr,
    pub enemies: Vec<FollowerIr>,
    pub obstacles: Vec<ObstacleIr>,
    /// The player's `Scripts` struct has TSL's `OnAccelerate` field (set
    /// or not): the ARE is TSL's.
    pub tsl: bool,
    /// Where the LYT places each track and obstacle model, lowercase.
    pub tracks: Vec<(String, [f32; 3])>,
    pub obstacle_places: Vec<(String, [f32; 3])>,
}

/// Anything that follows a track: the player, an enemy (a swoop's
/// acceleration pad, a Sith fighter).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FollowerIr {
    pub track: String,
    pub num_loops: i32,
    pub sphere_radius: f32,
    pub invince_period: f32,
    pub hit_points: i32,
    pub max_hps: i32,
    pub bump_damage: i32,
    pub death_sound: String,
    pub engine_sound: String,
    /// (model, `RotatingModel`).
    pub models: Vec<(String, bool)>,
    /// Its `Scripts` struct: (hook, script), the empty ones left out.
    pub scripts: Vec<(String, String)>,
    pub camera: String,
    pub camera_rotate: bool,
    pub min_speed: f32,
    pub max_speed: f32,
    pub accel_secs: f32,
    pub start_offset: [f32; 3],
    pub tunnel_infinite: [f32; 3],
    pub tunnel_pos: [f32; 3],
    pub tunnel_neg: [f32; 3],
    pub target_offset: [f32; 3],
    /// An enemy the player passes through, its hit the player's
    /// `OnHitFollower` (a swoop's pad).
    pub trigger: bool,
    pub gun_banks: Vec<GunBankIr>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GunBankIr {
    pub id: u32,
    pub gun_model: String,
    pub fire_sound: String,
    pub damage: i32,
    pub lifespan: f32,
    pub bullet_model: String,
    pub rate_of_fire: f32,
    pub collision_sound: String,
    pub speed: f32,
    /// Whom its bullets hit: 1 the player, 2 enemies, 3 both [verify].
    pub target_type: u32,
    pub inaccuracy: f32,
    pub sensing_radius: f32,
    pub horiz_spread: f32,
    pub vert_spread: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObstacleIr {
    /// Its model, placed where the LYT puts an obstacle of that name.
    pub name: String,
    pub scripts: Vec<(String, String)>,
}

/// `<label>_X`, `_Y`, `_Z`, or `<label>XPos` style triples by `names`.
fn triple(node: &GffStruct, names: [&str; 3]) -> [f32; 3] {
    names.map(|name| node.float(name).unwrap_or(0.0))
}

fn scripts(node: &GffStruct) -> Vec<(String, String)> {
    node.child("Scripts").map_or_else(Vec::new, |scripts| {
        crate::module::scripts(scripts)
            .into_iter()
            .filter(|(_, script)| !script.trim().is_empty())
            .collect()
    })
}

fn gun_bank(node: &GffStruct) -> GunBankIr {
    let bullet = node.child("Bullet");
    let float = |label| bullet.and_then(|b| b.float(label)).unwrap_or(0.0);
    let resref = |label| {
        bullet
            .and_then(|b| b.resref(label))
            .unwrap_or_else(String::new)
    };
    GunBankIr {
        id: node.uint("BankID").unwrap_or(0),
        gun_model: node.resref("Gun_Model").unwrap_or_else(String::new),
        fire_sound: node.resref("Fire_Sound").unwrap_or_else(String::new),
        damage: bullet.and_then(|b| b.int("Damage")).map_or(0, |v| v as i32),
        lifespan: float("Lifespan"),
        bullet_model: resref("Bullet_Model"),
        rate_of_fire: float("Rate_Of_Fire"),
        collision_sound: resref("Collision_Sound"),
        speed: float("Speed"),
        target_type: bullet.and_then(|b| b.uint("Target_Type")).unwrap_or(0),
        inaccuracy: node.float("Inaccuracy").unwrap_or(0.0),
        sensing_radius: node.float("Sensing_Radius").unwrap_or(0.0),
        horiz_spread: node.float("Horiz_Spread").unwrap_or(0.0),
        vert_spread: node.float("Vert_Spread").unwrap_or(0.0),
    }
}

fn follower(node: &GffStruct) -> FollowerIr {
    let sounds = node.child("Sounds");
    let sound =
        |label| sounds.map_or_else(String::new, |s| s.resref(label).unwrap_or_else(String::new));
    FollowerIr {
        track: node.resref("Track").unwrap_or_else(String::new),
        num_loops: node.int("Num_Loops").map_or(0, |v| v as i32),
        sphere_radius: node.float("Sphere_Radius").unwrap_or(0.0),
        invince_period: node.float("Invince_Period").unwrap_or(0.0),
        hit_points: node.int("Hit_Points").map_or(0, |v| v as i32),
        max_hps: node.int("Max_HPs").map_or(0, |v| v as i32),
        bump_damage: node.int("Bump_Damage").map_or(0, |v| v as i32),
        death_sound: sound("Death"),
        engine_sound: sound("Engine"),
        models: node
            .list("Models")
            .iter()
            .map(|model| {
                (
                    model.resref("Model").unwrap_or_else(String::new),
                    model.uint("RotatingModel").is_some_and(|v| v != 0),
                )
            })
            .filter(|(model, _)| !model.is_empty())
            .collect(),
        scripts: scripts(node),
        camera: node.resref("Camera").unwrap_or_else(String::new),
        camera_rotate: node.uint("CameraRotate").is_some_and(|v| v != 0),
        min_speed: node.float("Minimum_Speed").unwrap_or(0.0),
        max_speed: node.float("Maximum_Speed").unwrap_or(0.0),
        accel_secs: node.float("Accel_Secs").unwrap_or(0.0),
        start_offset: triple(node, ["Start_Offset_X", "Start_Offset_Y", "Start_Offset_Z"]),
        tunnel_infinite: node.vector("TunnelInfinite").unwrap_or([0.0; 3]),
        tunnel_pos: triple(node, ["TunnelXPos", "TunnelYPos", "TunnelZPos"]),
        tunnel_neg: triple(node, ["TunnelXNeg", "TunnelYNeg", "TunnelZNeg"]),
        target_offset: triple(
            node,
            ["Target_Offset_X", "Target_Offset_Y", "Target_Offset_Z"],
        ),
        trigger: node.uint("Trigger").is_some_and(|v| v != 0),
        gun_banks: node.list("Gun_Banks").iter().map(gun_bank).collect(),
    }
}

impl MiniGameIr {
    /// The ARE's minigame (`are` its root), if it has one.
    pub fn read(are: &GffStruct, layout: &Layout) -> Option<MiniGameIr> {
        let node = are.child("MiniGame")?;
        let mouse = node.child("Mouse").map(|mouse| {
            (
                [
                    mouse.int("AxisX").map_or(0, |v| v as u32),
                    mouse.int("AxisY").map_or(0, |v| v as u32),
                ],
                [
                    mouse.uint("FlipAxisX").is_some_and(|v| v != 0),
                    mouse.uint("FlipAxisY").is_some_and(|v| v != 0),
                ],
            )
        });
        let places = |placed: &[fmt_odyssey::lyt::Placed]| {
            placed
                .iter()
                .map(|placed| (placed.model.clone(), placed.position))
                .collect()
        };
        Some(MiniGameIr {
            kind: node.int("Type").map_or(0, |v| v as u32),
            near_clip: node.float("Near_Clip").unwrap_or(0.0),
            far_clip: node.float("Far_Clip").unwrap_or(0.0),
            view_angle: node.float("CameraViewAngle").unwrap_or(0.0),
            use_inertia: node.uint("UseInertia").is_some_and(|v| v != 0),
            do_bumping: node.uint("DoBumping").is_some_and(|v| v != 0),
            movement_per_sec: node.float("MovementPerSec").unwrap_or(0.0),
            lateral_accel: node.float("LateralAccel").unwrap_or(0.0),
            music: node.resref("Music").unwrap_or_else(String::new),
            mouse,
            player: node
                .child("Player")
                .map_or_else(FollowerIr::default, follower),
            enemies: node.list("Enemies").iter().map(follower).collect(),
            obstacles: node
                .list("Obstacles")
                .iter()
                .map(|obstacle| ObstacleIr {
                    name: obstacle.resref("Name").unwrap_or_else(String::new),
                    scripts: scripts(obstacle),
                })
                .collect(),
            tsl: node
                .child("Player")
                .and_then(|player| player.child("Scripts"))
                .is_some_and(|scripts| {
                    scripts
                        .fields
                        .iter()
                        .any(|field| field.label == b"OnAccelerate")
                }),
            tracks: places(&layout.tracks),
            obstacle_places: places(&layout.obstacles),
        })
    }
}
