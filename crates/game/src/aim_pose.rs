//! A character aiming a blaster raises it: a pistol held out at cheek
//! level, a rifle with its stock at the shoulder.
//!
//! - **The raise:** the body holds the last frame of its gun's shot
//!   (`cast::Figure::raise`), where JA's arms leave the gun after firing.
//!   The arms are then turned further by the wield's corrections
//!   (`corrections`). One pistol is held out at arm's length, its sights
//!   level with the cheek. A pistol in each hand is held up the same way,
//!   side by side. A rifle, a repeating blaster or any other gun
//!   that is not a pistol is held with both hands at the shoulder.
//! - **Where the gun points:** the hand turns the gun so its barrel runs
//!   to the point it aims at (`cast::gun_aim`). The player's own gun, seen
//!   from behind, aims at the point under the crosshair (`sighted`), where
//!   its bolts go. The camera stands beside the shoulder, so the barrel
//!   meets the crosshair's line there rather than running beside it. After
//!   the breathing and the firing motion turn the chest, a raised gun is
//!   turned back onto that point (`cast::reaim`). A rifle raised to the
//!   shoulder is not turned about the wrist: its stock is placed in the
//!   shoulder and the arm reaches to it (`asset_ja::rig::Aim::butt`), and
//!   the chest bows toward a target above or below. Each gun model has its
//!   own point that goes in the shoulder, read from its shape
//!   (`weapons::GunShape::seat`): a gun with a short stock, such as the
//!   blaster carbine, is held further out, and every gun's rear end stands
//!   as high beside the cheek as the blaster rifle's stock. On a body whose
//!   head hangs higher or further in from its shoulder than Carth's, the
//!   stock goes as much higher or further in. The support hand takes
//!   the gun again after the turns that came after the arms were posed
//!   (`cast::regrip`).
//! - **The head:** as the gun comes up, the head turns to look down the gun
//!   at the point it aims at, and at full raise it looks straight at that
//!   point (`cast::Head::turn`). With one pistol or a rifle it also leans
//!   toward the gun side (`head_tilt_deg`). With a rifle the head first
//!   comes down onto the stock (`cheek_weld_deg`) and rides back with it in
//!   a shot. The body the player looks through in first person keeps its
//!   head upright.
//! - **How fast:** a player's body follows the sim's raise level
//!   (`Shooting::aim_share`), eased (`eased`). The gun goes up over 200 ms
//!   and down over 150 ms, the sim's own times, and it never jumps.
//! - **Other characters:** the view does not ask the sim how far they
//!   aim. Each of their shots raises their guns, and they keep them up
//!   until `OTHERS_HOLD_MS` after their last shot. They also raise them
//!   while they charge a Power Blast or steady a Sniper Shot.
//! - **The legs** are not touched, so a character that walks while aiming
//!   keeps walking.
//!
//! The view alone: the sim's skeleton holds JA's pose.

use bevy::math::Vec3;
use collision::UNIT;
use collision::{Bodies, CollisionWorld as _, Extents};
use sim::combat::specials::Shooting;
use sim::motion::MotionState;
use sim::{Instances, SimId, Snapshot};

/// The nearest a sighted point stands along the crosshair's line (units, 1
/// metre). A wall or a body nearer than that would swing the barrel far to
/// the side, so the gun aims a metre out along the line instead.
pub(crate) const NEAREST_SIGHT: f32 = 1.0 / UNIT;

/// The point under the player's crosshair (units, game space): where the
/// crosshair's line (`sim::combat::mind::crosshair`, along `player`'s view)
/// first meets the level (`level` with the doors as `states` has them) or
/// a living body of `snapshot` other than `me`. A line that meets nothing
/// ends `sim::combat::weapons::HITSCAN_RANGE` out. The sim sends the
/// player's bolts from the muzzle to this same point
/// (`sim::world::firing`), so a barrel aimed at it points where the shots
/// go. The point is never nearer than `NEAREST_SIGHT` along the line. With
/// the camera looking steeply down, the guns aim further out and higher
/// than the crosshair's point (`held_aim`).
pub(crate) fn sighted(
    (level, states): (&sim::Level, Option<&[u8]>),
    snapshot: &Snapshot,
    me: SimId,
    player: &MotionState,
) -> Vec3 {
    let objects = Instances::in_snapshot(level, snapshot);
    let bodies: Vec<collision::Body> = (snapshot.entities.iter())
        .filter(|e| e.id != me && e.fighter.is_some_and(|f| f.alive()))
        .filter_map(|e| {
            let ps = e.player?;
            Some(sim::body(e.id, &ps, sim::bulk_of(objects.of_entity(e))))
        })
        .collect();
    let from = Vec3::from_array(sim::combat::mind::crosshair(player));
    let forward = Vec3::from_array(sim::math::angle_vectors(player.view).0);
    let far = from + forward * sim::combat::weapons::HITSCAN_RANGE;
    let met = level.with_unremembered(states, |world| {
        Bodies::new(world, &bodies)
            .skipping(me.0 | sim::BODY)
            .trace(
                (from * UNIT).to_array(),
                (far * UNIT).to_array(),
                &Extents::POINT,
                sim::SHOT,
            )
    });
    let along = (Vec3::from_array(met.end) / UNIT - from).dot(forward);
    held_aim(
        from + forward * along.max(NEAREST_SIGHT),
        (from, Vec3::from_array(player.origin)),
        forward,
    )
}

/// The nearest (units, 2 metres) a sighted point stands ahead of the
/// player's feet, along the way the player faces (`held_aim`). With the
/// camera looking steeply down, the crosshair meets the floor at the feet. A
/// gun aimed there points straight down or back at the body: the arm curls
/// up beside the head, and the gun turns on its side, so its scope shows
/// beside its barrel from above.
pub(crate) const LEAST_AHEAD: f32 = 2.0 / UNIT;

/// The furthest (degrees) a sighted point stands below the level of the
/// crosshair's line's start, seen from there (`held_aim`). A pair of pistols
/// aimed further down is held close together in front of the hips, and seen
/// from the camera above the arms look crossed. At 30 degrees a gun held at
/// the hip points about 20 degrees below level, close to its pose with the
/// camera level.
pub(crate) const STEEPEST_BELOW_DEG: f32 = 30.0;

/// `point`, where the guns aim, kept in front of the body and not too far
/// below the eyes. It is moved straight out along the level of `forward`
/// until it stands at least `LEAST_AHEAD` ahead of `origin`. Then it is
/// raised until it stands no more than `STEEPEST_BELOW_DEG` below the level
/// of `from`, the start of the crosshair's line. The bolts still fly to the
/// crosshair's point. Only the barrels aim elsewhere, and only while the
/// camera looks steeply down.
fn held_aim(point: Vec3, (from, origin): (Vec3, Vec3), forward: Vec3) -> Vec3 {
    let level = (forward * Vec3::new(1.0, 1.0, 0.0)).normalize_or_zero();
    let ahead = (point - origin).dot(level);
    let point = point + level * (LEAST_AHEAD - ahead).max(0.0);
    let flat = ((point - from) * Vec3::new(1.0, 1.0, 0.0)).length();
    let lowest = from.z - flat * STEEPEST_BELOW_DEG.to_radians().tan();
    point.with_z(point.z.max(lowest))
}

/// How the player's own gun, seen from behind, lines up with the point
/// under its crosshair as one frame draws it (game space, metres), for the
/// dumps (`play.sight`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    /// How far the gun is raised, from 0 at the hip to 1 fully raised.
    pub raise: f32,
    /// Where the gun's muzzle is drawn.
    pub muzzle: Vec3,
    /// The point under the crosshair (`sighted`).
    pub target: Vec3,
    /// The angle (degrees) between the barrel and the line from the muzzle
    /// to the target.
    pub barrel_off_deg: f32,
    /// The angle (degrees) between the way the head faces and the barrel.
    pub head_off_gun_deg: Option<f32>,
    /// The angle (degrees) between the way the head faces and the line from
    /// the head to the target.
    pub head_off_target_deg: Option<f32>,
    /// How far (metres) the last shot pushes the gun back along its barrel
    /// now, and how far (degrees) it turns the gun-side shoulder back
    /// (`recoil::push`).
    pub push_back_m: f32,
    pub shoulder_deg: f32,
    /// Where the middle of the stock's rear end stands from the gun-side
    /// shoulder joint (metres): ahead along the aim (level), up, and to the
    /// right of the aim.
    pub stock_m: Option<[f32; 3]>,
    /// How far (metres) to the right of the body's middle the stock's rear
    /// end and the muzzle stand, square to the aim (level). The body's
    /// middle is the upright plane along the aim through the point halfway
    /// between the shoulder joints.
    pub aside_m: Option<[f32; 2]>,
    /// How far (metres) the support hand's palm is from where it holds the
    /// gun (`support_palm`).
    pub support_gap_m: Option<f32>,
    /// The angle (degrees) between the way the face looks (its own node
    /// under the head's, which the head's clips turn) and the line from it
    /// to the target.
    pub face_off_target_deg: Option<f32>,
    /// How far (degrees) the face's top leans to the right of the upright
    /// plane along its look.
    pub face_roll_deg: Option<f32>,
    /// Where the face's node stands from the gun's seat in the shoulder
    /// (`weapons::GunShape::seat`, metres): ahead along the aim (level), up,
    /// and to the right of the aim.
    pub face_from_seat_m: Option<[f32; 3]>,
    /// How far (metres) the bottom of the face's jaw stands over the gun's
    /// seat in the shoulder, along the way the gun's top faces
    /// (`cheek_drop`).
    pub jaw_over_seat_m: Option<f32>,
}

/// Where the palm of a hand touches what it holds (metres): halfway from
/// the wrist to the middle of the knuckles, then `PALM_DEPTH` out toward
/// the side its fingertips curl to (`tips`, their middle).
pub(crate) fn support_palm(wrist: Vec3, knuckles: Vec3, tips: Vec3) -> Vec3 {
    let along = (knuckles - wrist).normalize_or_zero();
    let curl = tips - knuckles;
    let face = (curl - along * curl.dot(along)).normalize_or_zero();
    wrist.lerp(knuckles, 0.5) + face * PALM_DEPTH
}

/// How far (metres) a palm's face stands out from the bones of the hand, as
/// `asset_ja::rig::Aim::support` places it.
const PALM_DEPTH: f32 = 0.02;

/// How a character holds the guns it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wield {
    Pistol,
    Pair,
    Rifle,
}

/// The wield of a body by its KOTOR wield number (`stance::wield`): 5 one
/// pistol, 6 a pistol in each hand. Every other gun is held as a rifle.
pub(crate) fn wield_of(wield: u8) -> Wield {
    match wield {
        5 => Wield::Pistol,
        6 => Wield::Pair,
        _ => Wield::Rifle,
    }
}

/// The turns added to JA's bones over the held last frame of the shot, so
/// that the gun is held up to aim. Each is a bone of JA's skeleton
/// and its turn at full raise, in degrees (pitch, yaw, roll about the
/// bone's own axes, as `asset_ja::g2::Player::turn` takes them).
pub(crate) fn corrections(wield: Wield) -> &'static [(&'static str, [f32; 3])] {
    match wield {
        Wield::Pistol => PISTOL,
        Wield::Pair => PAIR,
        Wield::Rifle => RIFLE,
    }
}

// The turns were fitted so that each gun stands where a soldier holds it on
// the KOTOR body the player sees: JA's pose retargeted to Carth's body
// (`p_carthbb`), as the firing motion's probes pose it. Then they were
// checked in still pictures. The shoulders, the elbows and the chest turn.
// A bone's second angle turns it about its own length.

/// One pistol: the right arm is held out nearly straight ahead of the right
/// shoulder, bent about 20 degrees at the elbow, with the wrist 2 cm below
/// the shoulder's height and 54 cm in front of it. The pistol's sights are
/// then level with the cheek, a little below the eyes, and the gun is about
/// 15 cm to the right of the face's middle and 43 cm in front of it. The
/// chest turns toward the gun.
const PISTOL: &[(&str, [f32; 3])] = &[
    ("rhumerus", [18.5, -23.5, 11.0]),
    ("rradius", [-24.0, 0.5, 0.0]),
    ("thoracic", [0.0, 20.0, 0.0]),
];
/// A pistol in each hand: the chest turns square to the aim, and each arm
/// is held out nearly straight ahead of its own shoulder, bent about 20
/// degrees at the elbow. The sights are level with the cheeks, and the two
/// guns stand about 28 cm apart.
const PAIR: &[(&str, [f32; 3])] = &[
    ("rhumerus", [20.0, 3.5, 17.0]),
    ("rradius", [-33.5, 0.0, -20.5]),
    ("lhumerus", [64.0, -69.0, -93.0]),
    ("lradius", [-60.0, -25.0, 28.0]),
    ("thoracic", [0.0, 18.0, 0.0]),
];
/// A rifle: the arms are turned toward the shoulder here, and the rifle's
/// stock is then placed in the shoulder (`asset_ja::rig::Aim::butt`): on
/// Carth's body the middle of its rear end stands 12 cm in front of the
/// shoulder joint, 1 cm above it and 4.5 cm nearer the body's middle
/// (`recoil::posed`, `raised_rifle_at_shoulder`). The right elbow is bent
/// and hangs down. The chest turns the right side back, so the rifle lies
/// along the body. The left hand reaches under the barrel on its own
/// (`asset_ja::rig::Aim::support`), so its arm takes no turn here.
const RIFLE: &[(&str, [f32; 3])] = &[
    ("rhumerus", [30.5, 38.0, 1.5]),
    ("rradius", [31.5, 0.5, -53.0]),
    ("thoracic", [0.0, -44.5, 0.0]),
];

/// How far (degrees) a character's head leans toward the gun side at full
/// raise, as someone lining up a sight does. A character with one pistol
/// or a rifle leans its head 7 degrees. A character with a pistol in each
/// hand looks straight between them and does not lean its head.
pub(crate) fn head_tilt_deg(wield: Wield) -> f32 {
    match wield {
        Wield::Pistol | Wield::Rifle => 7.0,
        Wield::Pair => 0.0,
    }
}

/// How far (degrees) a character's head leans toward the gun side and bows
/// forward at full raise to rest its cheek on a rifle's stock, turning
/// where the head hangs on the neck (`weld_turn`). The head then still
/// looks at the point the gun aims at. A character with a pistol or a pair
/// does not bow its head.
pub(crate) fn cheek_weld_deg(wield: Wield) -> Option<[f32; 2]> {
    match wield {
        Wield::Rifle => Some(RIFLE_WELD_DEG),
        Wield::Pistol | Wield::Pair => None,
    }
}

/// A rifle's cheek weld (`cheek_weld_deg`): the lean toward the gun side
/// and the bow forward (degrees).
const RIFLE_WELD_DEG: [f32; 2] = [24.0, 15.0];

/// The lowest point of a head model's face, in the space of its face node
/// (`face`, an index of `ir`'s nodes) at rest: the lowest point of the
/// meshes that node moves most, the bottom of the jaw. None for a head
/// with no such point.
pub(crate) fn jaw_of(ir: &asset_odyssey::model::ModelIr, face: usize) -> Option<Vec3> {
    let world = |node: usize| ir.nodes.get(node).map(|n| crate::scene::to_mat4(&n.world));
    let face_world = world(face);
    let mut lowest: Option<Vec3> = None;
    for mesh in &ir.meshes {
        let Some(place) = world(mesh.node) else {
            continue;
        };
        for (index, position) in mesh.positions.iter().enumerate() {
            let mover = match &mesh.skin {
                None => Some(mesh.node),
                Some(skin) => skin
                    .vertex_joints
                    .get(index)
                    .zip(skin.vertex_weights.get(index))
                    .and_then(|(joints, weights)| {
                        let slot = (0..4).max_by(|&a, &b| weights[a].total_cmp(&weights[b]))?;
                        skin.joints.get(usize::from(joints[slot])).copied()
                    }),
            };
            if mover != Some(face) {
                continue;
            }
            let point = place.transform_point3(Vec3::from_array(*position));
            if lowest.is_none_or(|low| point.z < low.z) {
                lowest = Some(point);
            }
        }
    }
    Some(face_world?.inverse().transform_point3(lowest?))
}

/// The highest (metres) the bottom of a head's jaw may stand over a
/// rifle's seat in the shoulder, along the way the rifle's top faces, once
/// the cheek weld has turned the head (`cheek_drop`). On Carth's body
/// (`p_carthbb` with the head `p_carthh`) the jaw stands from 1.3 cm under
/// the seat to 0.84 cm over it, aimed from 25 degrees up to 25 degrees down
/// and through a shot, so his head never comes down further. A player
/// woman's (`pfbal` with `pfha01`) stands 2.4 to 6.5 cm over it.
const JAW_OVER_SEAT: f32 = 0.009;
/// The furthest (metres) a head comes down onto a rifle's stock past the
/// cheek weld's turn (`cheek_drop`).
const CHEEK_DROP_MOST: f32 = 0.06;

/// How far (metres) a head comes down past the cheek weld's turn so its
/// cheek rests on a rifle's stock, its jaw standing `over` (metres) over
/// the rifle's seat in the shoulder along the way the rifle's top faces:
/// as far as that is more than `JAW_OVER_SEAT`, and at most
/// `CHEEK_DROP_MOST`. The cheek weld's turn was fitted on Carth's body. A
/// head that hangs higher over the stock than his, or whose jaw sits higher
/// in its own model, still comes down onto the stock.
pub(crate) fn cheek_drop(over: f32) -> f32 {
    (over - JAW_OVER_SEAT).clamp(0.0, CHEEK_DROP_MOST)
}

/// The cheek weld's turn in the model's space for a head that looks
/// `toward` (only its level part counts), with the weld's `[lean, bow]`
/// (degrees) taken `share` of the way (how far the gun is raised): the top
/// of the head leans to the right of that way and bows forward along it.
pub(crate) fn weld_turn(toward: Vec3, [lean, bow]: [f32; 2], share: f32) -> bevy::math::Quat {
    use bevy::math::Quat;
    let forward = (toward * Vec3::new(1.0, 1.0, 0.0)).normalize_or(Vec3::Y);
    let right = forward.cross(Vec3::Z);
    Quat::from_axis_angle(forward, (lean * share).to_radians())
        * Quat::from_axis_angle(right, -(bow * share).to_radians())
}

/// The raise share (0 to 1) eased in and out: Perlin's smootherstep, which
/// starts and ends with no speed and no change of speed.
pub(crate) fn eased(share: f32) -> f32 {
    let s = share.clamp(0.0, 1.0);
    s * s * s * (s * (s * 6.0 - 15.0) + 10.0)
}

/// How long another character keeps its guns up after its last shot (ms).
pub(crate) const OTHERS_HOLD_MS: f64 = 600.0;

/// How far a body's guns are held up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lift {
    /// Not held up.
    Down,
    /// Held up, rising over a fixed time from when they went up.
    Timed,
    /// Another character's, held up after its shots. It rises over the
    /// same fixed time, from when the shot that raised it began.
    AfterShot,
    /// Held up as far as this share (0 to 1, eased), the sim's raise.
    Level(f32),
}

/// How far a body's guns are held up at `now_ms`. A body a seat acts
/// through (`seat`) follows the sim's raise while the gun is raised at
/// all. Any body has a timed raise while it charges a Power Blast or
/// steadies a Sniper Shot. Any other body also holds its guns up from each
/// of its shots until `OTHERS_HOLD_MS` after its last one (`last_shot_ms`).
pub(crate) fn lift(
    seat: bool,
    shooting: &Shooting,
    last_shot_ms: Option<f64>,
    now_ms: f64,
) -> Lift {
    let after_shot =
        !seat && last_shot_ms.is_some_and(|shot| (0.0..OTHERS_HOLD_MS).contains(&(now_ms - shot)));
    if seat && shooting.aim > 0 {
        Lift::Level(eased(shooting.aim_share()))
    } else if shooting.charging_from.is_some() {
        Lift::Timed
    } else if after_shot {
        Lift::AfterShot
    } else {
        Lift::Down
    }
}
