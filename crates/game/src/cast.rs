//! The characters `kjk play` draws: every player and creature in the
//! presented snapshots that has a body in the scene, standing where its
//! snapshots put it and animated from what they say it plays (render reads
//! nothing else).
//!
//! - A humanoid (the play body; creatures with the humanoid skeleton,
//!   `viewer::CreatureView`) is animated in a Ghoul2 player: a living
//!   fighter's legs and torso play as the sim's skeleton plays them (the
//!   snapshots carry its `Posed`: start, speed, the server's blends), so its
//!   blades are drawn where they cut (QUIRKS.md Q-0083); anyone else's (and
//!   everyone's with `quirks`) start at the snapshot's time, blended as the
//!   cgame blends them. `asset_ja::angles` turns the legs and spine with its
//!   view, velocity and ground; the pose is retargeted onto the KOTOR body
//!   (`asset_ja::rig::Retargeter`), and a hand that grips a blade reaches
//!   for the sim's (`Reaching`, `asset_ja::rig::Reach`). A death animation
//!   is not looped: it plays to its end and holds. A script's animation
//!   draws the body's own KOTOR clip over that pose (`acting`).
//! - Any other creature (droids, beasts) plays its own KOTOR clips, picked
//!   from its state: standing, walking or running (by its legs' animation
//!   and speed), and once dead its dying clip to the end, then its dead
//!   one, looped (reone's `die` and `dead`). Standing in a fight, one that
//!   is not a creature model (HK-47) holds its wield's ready stance
//!   (`g<wield>r1`); each attack plays its clip over the pose
//!   (`acting::Attack`). A clip that takes over fades in from where the
//!   last one stopped over its transition time.
//! - Each is placed between the snapshots around the moment and turned to
//!   its legs' yaw.
//! - Each holds its weapons and lights its blades as `weapons` says; a
//!   humanoid with a blade out grips it JA's way (`BodyRig::gripped`).
//! - A character wearing a disguise, or a creature whose body armour
//!   changed (`made::guise`), is drawn in the disguise's body
//!   (`made` reads and spawns it, `disguise` swaps it in): its figure
//!   carries on there, its weapons hang from the new hands, and its own
//!   body waits hidden until it takes the disguise off.
//! - A humanoid with a gun out holds it up and aims it (`gun_aim`,
//!   `asset_ja::rig::Aim`) while its torso plays the gun's ready or shot
//!   animation: the barrel down its shots' way to where the bolt drawn from
//!   the muzzle meets it (`combat_fx::CONVERGE`), upright, the other hand
//!   under it, kicked by each shot. With its gun not readied (out of a
//!   fight, `weapons::armed`) the torso plays the legs'
//!   part instead of JA's ready pose, blending out quickly as it is drawn,
//!   slowly as it goes away (QUIRKS.md Q-0085).
//!
//! Limits, for the frame's cost. Every character follows its snapshots and
//! steers every step, which is cheap. A pose (Ghoul2 and the retarget, or
//! the clip) is the costly part. It is worked out every frame for the
//! `POSED` characters nearest the camera within `POSE_RANGE`, and the
//! camera's own player is always among them. It is also worked out every
//! frame for the `POSED_IN_VIEW` nearest of the rest within `FAR_RANGE`
//! that the camera sees. A small one among those is posed at most once
//! every `SMALL_GAP`, by the clock on the wall. The others within `FAR_RANGE` are posed in turn,
//! one frame in `FAR_EVERY`. Beyond `FAR_RANGE`, characters keep their last
//! pose while they move. The torso's `Motion` bolt, which needs a
//! pose of its own, is read only for posed characters whose legs and torso play
//! different animations (the only time the look uses it). Each distinct
//! body model is lined up with JA's skeleton once and kept for the run.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_ja::angles;
use asset_ja::events::{Events, Keyed, Passage};
use asset_ja::g2::{Bones, Local, Part, PartSet, Player as G2};
use asset_ja::rig::{Aim, Eased, Grip, Posed, Reach, Retargeter, Side, Stowed, audit_off};
use asset_odyssey::anim::AnimationIr;
use asset_odyssey::hooks;
use asset_odyssey::model::ModelIr;
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;
use collision::UNIT;
use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::Gla;
use script_ncs::vm::ObjectId;
use sim::combat::idle::Acting;
use sim::combat::saber::{Blocked, SaberRules, SaberState};
use sim::combat::sheet::Sheet;
use sim::combat::{Delivery, Fighter, Location, Weapon};
use sim::module::ObjectKind;
use sim::motion::{self, Anim, MotionState};
use sim::remote::Remote;
use sim::script::dialogue::Conversation;
use sim::skeleton::{Blade, Hand};
use sim::{EntityState, Instances, Kind, Seat, SimId, Snapshot};

use crate::acting;
use crate::aim_pose::{self, Lift};
use crate::draw_anim::DrawPose;
use crate::ground_lean::{self, Floor, Lean};
use crate::holdings::Holdings;
use crate::ragdolls::{self, Doll, Dying, Lengths, Ragdolls};
use crate::scene::{Animator, SceneModel, game_to_bevy};
use crate::stance;
use crate::transforms::{Staged, Transforms};
use crate::viewer::{CreatureClips, CreatureView, LoadedScene};
use crate::weapons::{self, Armed, Hands, Hide, Owner, Props, Sabers, Stow, Wielded};

/// How many characters are posed every frame, nearest the camera first.
pub const POSED: usize = 16;
/// How far from the camera a character is posed every frame (metres).
pub const POSE_RANGE: f32 = 30.0;
/// A far fight posed in turns stutters.
pub const POSED_IN_VIEW: usize = 64;
const SMALL_SHARE: f32 = 0.04;
/// Measured by the clock on the wall, not by the world's time. A world in
/// slow motion, or one held still, still draws at the full frame rate.
const SMALL_GAP: std::time::Duration = std::time::Duration::from_millis(12);
const STANDING_M: f32 = 1.8;
const IN_VIEW_RADIUS: f32 = 4.0;
/// The rest within `FAR_RANGE` (metres) take turns, each posed every
/// `FAR_EVERY`th frame: a held pose would stop a far fight mid-swing.
pub const FAR_RANGE: f32 = 150.0;
pub const FAR_EVERY: usize = 4;
/// Below this speed (units per second) a creature stands. It walks or runs
/// as the sim's legs animation says (as JA picks walk or run by the walk
/// button the scripts' moves press); with legs doing neither, below
/// `RUN_SPEED` (tuning, between JA's walk and run speeds) it walks.
pub const STILL_SPEED: f32 = 10.0;
const RUN_SPEED: f32 = 180.0;
/// How quickly a gun comes up to its aim (ms, the time constant): about
/// the blend JA's torso takes into its ready pose.
const AIM_EASE_MS: f32 = 60.0;
/// How long the torso blends as a gun comes out, the moment a fight starts
/// (a first shot is fired as it does), and as it goes away (ms); the aim
/// eases out over the latter.
const DRAW_BLEND_MS: f32 = 50.0;
const HOLSTER_BLEND_MS: f32 = 250.0;
/// How long after a shot the gun is held fully aimed whatever else plays
/// (ms): a shot always leaves along the barrel.
const SHOT_AIM_MS: f64 = 150.0;
/// Seconds a gripping hand takes to reach for the sim's blade as a cut
/// begins and to let go as it ends (`Figure::reach_share`): short, as JA's
/// swings are (an attack lasts about 0.3 s), the arm solved smoothly all
/// the while (`rig::bend`).
const REACH_EASE_S: f32 = 0.08;
/// Seconds a gripping hand takes, under the duel rules, to turn the drawn
/// blade fully the sim's way as a block, a bolt deflection or a saber lock
/// begins (`Figure::turn_share`): two or three frames, as these have no
/// lead-in to turn over.
const CONTACT_EASE_S: f32 = 0.03;
/// How far a gripping hand's wrist moves at most to put the drawn blade
/// where the sim's is while its holder is in a saber lock, under either
/// set of saber rules (metres). A lock plays JA's lock animations, which
/// JA made for JA's bodies. On KOTOR's bodies the hilts land up to half a metre from
/// where the sim's skeleton holds them (a double-bladed hilt the most), so
/// the blades that meet in the sim pass by each other as drawn.
const LOCK_REACH: f32 = 0.6;
/// Seconds a gripping hand takes to settle into a held stance's reach:
/// longer, the stance's pose coming in after a return (at `REACH_EASE_S`
/// the hand snapped up to `asset_ja::rig::REACH` across the chest).
const STANCE_EASE_S: f32 = 0.3;
/// How much a gripping hand reaches for the sim's blade in a held stance.
const STANCE_REACH: f32 = 1.0;
/// Where a gun's muzzle is held before the eyes looking through them
/// (`first_person`): ahead and right along the view, and straight down
/// (metres, `before_eyes`; tuning, the owner's approved look, as are the
/// blade's below).
const HOLD: [f32; 3] = [0.5, 0.14, 0.18];
/// Where a blade at rest is held before them: the right hand's hilt ahead,
/// right and down as a gun is (metres; the left's mirrored), the blade rising ahead,
/// outward and up, clear of the crosshair. It eases in and out over
/// `BLADE_EASE_MS` (the time constant) as swings end and start, the arm
/// reaching as far as `BLADE_REACH` (metres) for it.
const BLADE_HOLD: [f32; 3] = [0.42, 0.26, 0.25];
const BLADE_RISE: [f32; 3] = [0.45, 0.3, 0.85];
const BLADE_EASE_MS: f32 = 70.0;
const BLADE_REACH: f32 = 0.8;
/// A bolt turned while the blade is held at rest (a parry's move lets the
/// hold go, so only a block from the ready stance shows it): the blade is
/// across where it was turned the frame its sparks fly, and eases back over
/// `FLICK_MS` (the time constant).
const FLICK_MS: f32 = 160.0;

/// JA's humanoid skeleton and its sequences: what every figure plays.
pub struct Rig {
    pub gla: &'static Gla,
    /// By upper-case name.
    pub sequences: BTreeMap<String, Sequence>,
    /// Where the JA body's soles stand in its base pose (metres).
    pub ja_sole: f32,
    /// Reproduce the jolting playback and look quirks.
    pub quirks: bool,
    /// The JA body's hand bolts (`*r_hand`, `*l_hand`): where a weapon is
    /// held.
    pub hands: [Option<asset_ja::Bolt>; 2],
    /// The JA sabers the hands hold blades as.
    pub sabers: Sabers,
    /// The keyframed sounds of JA's animations (`animevents.cfg`).
    pub events: Arc<Events>,
}

/// A skeleton's rest pose: each node's parent, translation and rotation on
/// it.
pub type Rest = [(Option<usize>, Vec3, Quat)];

/// `ir`'s rest pose.
pub fn rest_of(ir: &ModelIr) -> Vec<(Option<usize>, Vec3, Quat)> {
    ir.nodes
        .iter()
        .map(|n| {
            let [x, y, z, w] = n.rotation;
            (
                n.parent,
                Vec3::from_array(n.translation),
                Quat::from_xyzw(x, y, z, w),
            )
        })
        .collect()
}

/// A KOTOR body lined up with JA's skeleton, and its nodes' rest pose.
pub struct BodyRig {
    pub(crate) retargeter: Retargeter<'static>,
    ir: &'static ModelIr,
    hold: Vec<(String, [f32; 4])>,
    /// Each node's parent, rest translation and rotation.
    pub rest: Vec<(Option<usize>, Vec3, Quat)>,
    pub animation_scale: f32,
    /// Its own clips out of a fight (`gait`), a player's body's.
    pub gait: crate::gait::GaitClips,
    /// A seat's body's scripted clips (`acting::clips`), by constant: what
    /// the engine plays on a player (a mine's disarm).
    pub acted: Vec<(u16, Arc<AnimationIr>)>,
    /// The nodes left as the last clip put them (`held_nodes`).
    pub held: Vec<bool>,
}

impl BodyRig {
    /// The same, moving out of a fight with `gait`'s clips.
    pub fn walking(self, gait: crate::gait::GaitClips) -> BodyRig {
        BodyRig { gait, ..self }
    }

    /// `hold`: rotations held on nodes JA does not move (the fingers).
    pub fn new(ir: &ModelIr, rig: &Rig, hold: Vec<(String, [f32; 4])>) -> BodyRig {
        // The retargeter borrows the model for as long as the run lasts.
        BodyRig::with_grips(Box::leak(Box::new(ir.clone())), rig, hold, &[])
    }

    fn with_grips(
        ir: &'static ModelIr,
        rig: &Rig,
        hold: Vec<(String, [f32; 4])>,
        grips: &'static [Grip],
    ) -> BodyRig {
        let retargeter = Retargeter::new(
            ir,
            rig.gla,
            &asset_ja::rig::Options {
                ground: None,
                grips: grips.iter().collect(),
                off_bolt: rig.hands[1].clone(),
                hold: hold.clone(),
                ja_sole: rig.ja_sole,
            },
        );
        let rest = rest_of(ir);
        BodyRig {
            retargeter,
            ir,
            hold,
            rest,
            animation_scale: ir.animation_scale,
            gait: crate::gait::GaitClips::default(),
            acted: Vec::new(),
            held: Vec::new(),
        }
    }

    /// The body with its scripted clips, its standing one telling which
    /// nodes it holds (`held_nodes`).
    pub fn with_acted(self, acted: Vec<(u16, Arc<AnimationIr>)>) -> BodyRig {
        let pause = acted
            .iter()
            .find(|(animation, _)| *animation == acting::LOOPING_PAUSE);
        let held = held_nodes(self.ir, pause.map(|(_, clip)| &**clip));
        BodyRig {
            acted,
            held,
            ..self
        }
    }

    /// The same body holding a blade JA's way in its right hand, and with
    /// `dual` a second in its left (`asset_ja::rig::Grip`): each hand turned
    /// so a KOTOR hilt at its weapon node points along `blade` (a JA hilt's
    /// blade direction) as JA's hand bolt points it, its fingers round the
    /// hilt (`gripping_hold`; `double`: a staff). `None` if the body or the
    /// JA body lacks a hand.
    pub fn gripped(
        &self,
        rig: &Rig,
        blade: [f32; 3],
        (double, dual): (bool, bool),
    ) -> Option<BodyRig> {
        let hands = if dual { 2 } else { 1 };
        let grips = (0..hands)
            .map(|hand| {
                let bolt = rig.hands[hand].as_ref()?;
                let grip = Grip::new(self.ir, rig.gla, bolt, blade, weapons::WEAPON_NODES[hand]);
                grip.ok()
                    .map(|grip| if double { grip.staff() } else { grip })
            })
            .collect::<Option<Vec<Grip>>>()?;
        // The retargeter borrows its grips for as long as the run lasts.
        let grips: &'static [Grip] = Box::leak(grips.into_boxed_slice());
        let hold = self.gripping_hold(double, dual);
        let gripped = BodyRig {
            held: self.held.clone(),
            ..BodyRig::with_grips(self.ir, rig, hold, grips)
        };
        Some(gripped.walking(self.gait.clone()))
    }

    /// The fingers holding hilts, from the first frames of KOTOR's ready
    /// stances (`acted`): the right hand's as its wield's holds them
    /// (`g2r1` a blade, `g3r1` a staff, `g4r1` one in each hand), the
    /// left's round a hilt (`g3r1`'s on a staff, else `g4r1`'s: the left
    /// holds the right's hilt too in JA's two-handed stances); the rest as
    /// `hold`.
    fn gripping_hold(&self, double: bool, dual: bool) -> Vec<(String, [f32; 4])> {
        let fingers = |wield: u16, side: char| {
            self.acted
                .iter()
                .find(|(animation, _)| *animation == acting::READY + wield)
                .into_iter()
                .flat_map(|(_, clip)| finger_hold(clip))
                .filter(move |(name, _)| name.to_ascii_lowercase().starts_with(side))
        };
        let right = match (double, dual) {
            (true, _) => 3,
            (_, true) => 4,
            _ => 2,
        };
        let left = if double { 3 } else { 4 };
        let mut hold = self.hold.clone();
        for (name, rotation) in fingers(right, 'r').chain(fingers(left, 'l')) {
            match hold.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                Some(held) => held.1 = rotation,
                None => hold.push((name, rotation)),
            }
        }
        hold
    }

    pub fn node_names(&self) -> Vec<&str> {
        self.retargeter.node_names()
    }

    /// The body model it was lined up for.
    pub fn model(&self) -> &'static ModelIr {
        self.ir
    }
}

/// The fingers' rotations in a KOTOR clip's first frame: a hand pose for a
/// retargeted body.
pub fn finger_hold(clip: &AnimationIr) -> Vec<(String, [f32; 4])> {
    clip.tracks
        .iter()
        .filter(|t| crate::viewer_rig::is_finger(&t.node))
        .filter_map(|t| {
            let q = asset_odyssey::anim::sample_orientation(&t.orientations, 0.0)?;
            Some((t.node.clone(), q))
        })
        .collect()
}

/// A character at a moment: the later snapshot's state, with the origin
/// and view blended between the two around it.
#[derive(Clone, Copy)]
pub struct Blended {
    pub state: MotionState,
    pub origin: [f32; 3],
    pub view: [f32; 3],
    pub alive: bool,
    /// Dead, not downed (I30 P11): no getting up.
    pub dead: bool,
    pub fighter: Option<Fighter>,
    /// When the snapshot that `state` and `fighter` come from was taken
    /// (sim ms).
    pub taken_ms: f64,
    /// Its saber away from the hand: where it is (units), blended.
    pub thrown_at: Option<[f32; 3]>,
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// A joined player's own character: drawn from its client's predictions
/// (`history`) at their own time `ms`, moved by the correction (units),
/// ahead of everyone else, who are drawn from the host's snapshots.
#[derive(Clone, Copy)]
pub struct Own<'a> {
    pub id: SimId,
    pub history: &'a [Arc<Snapshot>],
    pub ms: f64,
    pub correction: [f32; 3],
}

impl Own<'_> {
    fn corrected(&self, origin: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|axis| origin[axis] + self.correction[axis])
    }
}

fn own_of<'o, 'a>(own: Option<&'o Own<'a>>, id: SimId) -> Option<&'o Own<'a>> {
    own.filter(|own| own.id == id)
}

/// The character `id` at `ms` in `history`, or, the own character, from
/// its predictions.
pub fn blended_for(
    history: &[Arc<Snapshot>],
    id: SimId,
    ms: f64,
    own: Option<&Own<'_>>,
) -> Option<Blended> {
    match own_of(own, id) {
        Some(own) => {
            let mut at = blended(own.history, id, own.ms)?;
            at.origin = own.corrected(at.origin);
            Some(at)
        }
        None => blended(history, id, ms),
    }
}

fn origin_for(
    history: &[Arc<Snapshot>],
    id: SimId,
    ms: f64,
    own: Option<&Own<'_>>,
) -> Option<[f32; 3]> {
    match own_of(own, id) {
        Some(own) => Some(own.corrected(origin_at(own.history, id, own.ms)?)),
        None => origin_at(history, id, ms),
    }
}

/// The character `id` at `ms` in `history` (oldest first), as drawn: one
/// a destroy took out of the world stands as it went while it departs.
pub fn blended(history: &[Arc<Snapshot>], id: SimId, ms: f64) -> Option<Blended> {
    let (a, b, t) = snapshots_at(history, ms)?;
    let (ea, eb) = (a.drawn(id)?, b.drawn(id)?);
    let (pa, pb) = (ea.player?, eb.player?);
    let thrown = |e: &EntityState| e.fighter.and_then(|f| f.saber.thrown).map(|t| t.at);
    Some(Blended {
        state: pb,
        origin: lerp(pa.origin, pb.origin, t),
        view: view_between(&pa, &pb, t),
        alive: alive(eb),
        dead: eb.fighter.as_ref().is_some_and(|f| f.dead()),
        fighter: eb.fighter,
        taken_ms: b.time_ms as f64,
        thrown_at: thrown(eb).map(|at| thrown(ea).map_or(at, |from| lerp(from, at, t))),
    })
}

fn view_between(from: &MotionState, to: &MotionState, t: f32) -> [f32; 3] {
    std::array::from_fn(|i| from.view[i] + angles::wrap_180(to.view[i] - from.view[i]) * t)
}

fn alive(entity: &EntityState) -> bool {
    entity.fighter.as_ref().is_none_or(|f| f.alive())
}

struct Steered<'a> {
    view: [f32; 3],
    state: &'a MotionState,
    alive: bool,
}

fn steered<'a>(
    history: &'a [Arc<Snapshot>],
    id: SimId,
    ms: f64,
    own: Option<&Own<'a>>,
) -> Option<Steered<'a>> {
    let (history, ms) = own_of(own, id).map_or((history, ms), |own| (own.history, own.ms));
    let (a, b, t) = snapshots_at(history, ms)?;
    let (ea, eb) = (a.drawn(id)?, b.drawn(id)?);
    let (pa, pb) = (ea.player.as_ref()?, eb.player.as_ref()?);
    Some(Steered {
        view: view_between(pa, pb, t),
        state: pb,
        alive: alive(eb),
    })
}

fn origin_at(history: &[Arc<Snapshot>], id: SimId, ms: f64) -> Option<[f32; 3]> {
    let (a, b, t) = snapshots_at(history, ms)?;
    let (pa, pb) = (a.drawn(id)?.player.as_ref()?, b.drawn(id)?.player.as_ref()?);
    Some(lerp(pa.origin, pb.origin, t))
}

fn snapshots_at(history: &[Arc<Snapshot>], ms: f64) -> Option<(&Snapshot, &Snapshot, f32)> {
    let after = history.iter().position(|s| s.time_ms as f64 >= ms);
    let (a, b) = match after {
        Some(0) => (history.first()?, history.first()?),
        Some(i) => (&history[i - 1], &history[i]),
        None => (history.last()?, history.last()?),
    };
    let span = (b.time_ms - a.time_ms) as f64;
    let t = if span > 0.0 {
        ((ms - a.time_ms as f64) / span) as f32
    } else {
        1.0
    };
    Some((a, b, t))
}

/// Seat `seat`'s remote droid at `ms` in `history`: its place, yaw and
/// pitch blended between the snapshots either side. None while it waits
/// out a hold (`Remote::hidden`) or has none.
pub fn remote_in(history: &[Arc<Snapshot>], seat: Seat, ms: f64) -> Option<Remote> {
    let (a, b, t) = snapshots_at(history, ms)?;
    let t = t.clamp(0.0, 1.0);
    let rb = b.remote(seat).filter(|remote| !remote.hidden)?;
    let ra = a.remote(seat).filter(|remote| !remote.hidden).unwrap_or(rb);
    let turned = |from: f32, to: f32| from + angles::wrap_180(to - from) * t;
    Some(Remote {
        at: lerp(ra.at, rb.at, t),
        yaw: turned(ra.yaw, rb.yaw),
        pitch: turned(ra.pitch, rb.pitch),
        ..*rb
    })
}

/// A remote droid as the camera follows it: a body standing at its place
/// (units), looking along its view, its eyes at its middle.
pub fn remote_body(remote: &Remote) -> Blended {
    let origin = remote.at.map(|metres| metres / UNIT);
    let mut state = MotionState::standing(origin, remote.yaw);
    state.eye_height = 0.0;
    Blended {
        state,
        origin,
        view: [remote.pitch, remote.yaw, 0.0],
        alive: true,
        dead: false,
        fighter: None,
        taken_ms: 0.0,
        thrown_at: None,
    }
}

/// An entity of a snapshot by id (entities are ordered by id).
/// A human's eyes over its soles (metres), where a body has no node to
/// say (JA's own body's talk position, a free camera over an entry point,
/// an effect's head) [verify: K1's humans' `headconjure` stands about
/// 1.7 m up].
pub const HEAD_M: f32 = 1.7;

/// Where a character whose box stands at `origin` (game units) has its
/// soles: the box's bottom (metres), where its model's root stands.
pub fn soles(origin: [f32; 3]) -> Vec3 {
    Vec3::new(
        origin[0] * UNIT,
        origin[1] * UNIT,
        (origin[2] + motion::SOLE_Z) * UNIT,
    )
}

/// The `camerahook` node of the model hung on body model `body` (its
/// head), if one has it: `models` the scene's root and node entities by
/// index.
fn hung_camera(
    body: usize,
    scene: &LoadedScene,
    models: &[Option<(Entity, Vec<Entity>)>],
) -> Option<Entity> {
    scene.models.iter().enumerate().find_map(|(at, placed)| {
        placed.hook.filter(|(on, _)| *on == body)?;
        let node = placed.ir.node_index(hooks::CAMERA_HOOK)?;
        models.get(at)?.as_ref()?.1.get(node).copied()
    })
}

/// The top of body model `body`'s meshes and its head's (hung at the body's
/// head hook), at rest, over its root (metres); none without meshes. What
/// the body holds, hung at its hands, does not count.
fn model_height(body: usize, scene: &LoadedScene) -> Option<f32> {
    let body_model = scene.models.get(body)?;
    let head_hook = body_model.ir.node_index(hooks::HEAD_HOOK);
    let hung = scene
        .models
        .iter()
        .filter(|placed| {
            placed
                .hook
                .is_some_and(|(on, node)| on == body && Some(node) == head_hook)
        })
        .map(|placed| {
            let attach = placed
                .attach
                .as_ref()
                .map_or(Mat4::IDENTITY, crate::scene::to_mat4);
            mesh_top(placed, attach)
        });
    std::iter::once(mesh_top(body_model, Mat4::IDENTITY))
        .chain(hung)
        .flatten()
        .reduce(f32::max)
}

/// The highest point of `placed`'s meshes at rest, moved by `hung`.
fn mesh_top(placed: &crate::viewer::Placed, hung: Mat4) -> Option<f32> {
    placed
        .ir
        .meshes
        .iter()
        .flat_map(|mesh| {
            let world = hung * crate::scene::to_mat4(&placed.ir.nodes[mesh.node].world);
            mesh.positions
                .iter()
                .map(move |p| world.transform_point3(Vec3::from_array(*p)).z)
        })
        .reduce(f32::max)
}

/// The first character or object of `snapshot` tagged `tag` (without case).
pub fn tagged(snapshot: &Snapshot, objects: &Instances, tag: &str) -> Option<SimId> {
    snapshot
        .entities
        .iter()
        .find(|entity| is_tagged(entity, objects, tag))
        .map(|entity| entity.id)
}

/// Whether `entity` is an object of `objects` tagged `tag`, its case aside
/// (the games lowercase a tag as its object loads; a route or a console
/// names it as written).
pub fn is_tagged(entity: &EntityState, objects: &Instances, tag: &str) -> bool {
    objects
        .of_entity(entity)
        .is_some_and(|data| data.tag.eq_ignore_ascii_case(tag))
}

/// Whether a character is a droid: its sheet's race (the template's
/// `Race`, `racialtypes.2da`'s droid), which decides its body's metal
/// (`ragdolls`) and that hits spark on it rather than bleed (`combat_fx`;
/// every K1 droid appearance's `bloodcolr` is the sparks' `S`).
pub fn droid(entity: &EntityState, objects: &Instances) -> bool {
    sheet(entity, objects).race == sim::combat::sheet::races::DROID
}

pub fn entity(snapshot: &Snapshot, id: SimId) -> Option<&EntityState> {
    snapshot
        .entities
        .binary_search_by_key(&id, |e| e.id)
        .ok()
        .map(|i| &snapshot.entities[i])
}

/// The weapon a character has out: its sheet's in the hand its fighter
/// uses, as the sim reads it.
pub fn weapon(entity: &EntityState, objects: &Instances) -> Weapon {
    let sheet = sheet(entity, objects);
    let hand = entity.fighter.map_or(0, |f| usize::from(f.hand.min(1)));
    sheet.weapons[hand].unwrap_or(sheet.main_weapon())
}

/// A character's sheet as the sim reads it: the one in its entity (a
/// player's, a creature's once its equipment changed or it was made in
/// play), else its instance's (`objects`, the snapshot's), a commoner's
/// without either.
pub fn sheet(entity: &EntityState, objects: &Instances) -> Sheet {
    entity
        .sheet
        .or_else(|| objects.of_entity(entity)?.sheet)
        .unwrap_or_else(Sheet::commoner)
}

/// The body `seat` plays and its sheet as the sim reads it (`sheet`): a
/// driven party member's from its instance until its equipment changes.
pub fn body_sheet<'a>(
    snapshot: &'a Snapshot,
    seat: Seat,
    level: &sim::Level,
) -> Option<(&'a EntityState, Sheet)> {
    let body = snapshot.body(seat)?;
    Some((body, worn_sheet(snapshot, body, level)))
}

/// `entity`'s sheet as the sim reads it (`sheet`).
pub fn worn_sheet(snapshot: &Snapshot, entity: &EntityState, level: &sim::Level) -> Sheet {
    match entity.sheet {
        Some(worn) => worn,
        None => sheet(entity, &Instances::in_snapshot(level, snapshot)),
    }
}

/// The party as `seat` sees it, as the bar, the HUD, the pages and their 3D
/// view list it: its body (`Snapshot::body`: its avatar, the companion it
/// drives, I31, or the one leading) first. The party's own order is the
/// seat's own character while in the party, then the companions that follow
/// the seat (`Snapshot::follows`: no TSL puppet, Bao-Dur's remote), by id;
/// it is turned to start at the body (as KOTOR's leader
/// key turns it), so the next after the body is the next to lead.
pub fn party_of(snapshot: &Snapshot, seat: Seat) -> Vec<&EntityState> {
    let body = snapshot.body(seat);
    let own = snapshot.own(seat).filter(|own| snapshot.in_party(own));
    let followers = snapshot
        .entities
        .iter()
        .filter(|entity| snapshot.follows(entity, seat));
    let mut party: Vec<&EntityState> = own.into_iter().chain(followers).collect();
    if let Some(body) = body {
        match party.iter().position(|member| member.id == body.id) {
            Some(at) => party.rotate_left(at),
            None => party.insert(0, body),
        }
    }
    party
}

/// The characters the in-game pages show for `seat` (`party_of`). In one
/// of the party's bases (`sim::script::party::BASES`), the first seat's
/// pages also show the companions standing there out of the party
/// (`Snapshot::waiting`), after the party. The player can then change their
/// equipment and look at their sheets, as in the original games. Only the
/// host dresses a companion no seat plays (I30 P14).
pub fn pages_of(snapshot: &Snapshot, seat: Seat) -> Vec<&EntityState> {
    let mut party = party_of(snapshot, seat);
    if seat == Seat(0) {
        let waiting: Vec<&EntityState> = (snapshot.waiting.iter())
            .filter(|id| party.iter().all(|member| member.id != **id))
            .filter_map(|id| snapshot.get(*id))
            .collect();
        party.extend(waiting);
    }
    party
}

/// JA's `WeaponReadyAnim` for the weapon: what the torso holds standing
/// ready (sabers and KOTOR's melee weapons, which fight with the saber
/// moves, `BOTH_STAND2`; a gun's by how many hands hold it,
/// `sim::motion::gun_ready`).
fn ready_anim(weapon: Weapon) -> &'static str {
    match weapon.delivery {
        Delivery::Saber(_) | Delivery::Melee => "BOTH_STAND2",
        Delivery::Blaster(_) => sim::motion::gun_ready(weapon.one_handed()).name(),
        Delivery::Baton | Delivery::Unarmed => "TORSO_WEAPONREADY3",
    }
}

/// How long a get-up blends in from the pose the body lay in (ms).
const RISE_BLEND_MS: f32 = 400.0;
/// The share of a get-up through which it keeps the turn that lays it
/// where the death left the body; it turns back to the body's facing over
/// the rest, as it stands.
const RISE_TURN_HELD: f32 = 0.35;

/// A body on JA's animations getting up from the death it lay in
/// (`sim::combat::death::getup`): the get-up the sim plays, how far it is
/// turned at its start (degrees, `getup_turn`), how long it lasts (ms),
/// which parts (legs, torso) have blended into it from the lying pose, and
/// when the legs began to (ms).
#[derive(Debug, Clone, Copy)]
struct Rising {
    anim: Anim,
    turn: f32,
    length_ms: f32,
    blended: [bool; 2],
    legs_from_ms: Option<f32>,
}

/// How a body lies at the first or `last` frame of `sequence`, in the GLA's
/// model space: the heading (degrees about its up axis) from its pelvis to
/// its head, its pelvis, and which way its chest faces (up when positive).
pub fn lying(gla: &Gla, sequence: &Sequence, last: bool) -> Option<(f32, Vec3, f32)> {
    let mut g2 = G2::new(gla);
    let bone = |name: &str| g2.bone(name);
    let (root, pelvis, head) = (bone("model_root")?, bone("pelvis")?, bone("cranium")?);
    let (right, left) = (bone("rhumerus")?, bone("lhumerus")?);
    // A sequence played backwards (negative rate) ends on its first frame.
    let (first, end) = (
        sequence.first_frame,
        sequence.first_frame + sequence.frame_count.max(1) - 1,
    );
    let frame = if last == (sequence.frames_per_second >= 0) {
        end
    } else {
        first
    };
    let held = asset_ja::g2::Timed {
        start_frame: frame,
        end_frame: frame + 1,
        speed: 1.0,
        start_ms: 0.0,
        looping: false,
        blend: None,
    };
    g2.adopt(root, &held);
    let pose = g2.pose(0.0);
    let at = |bone| g2.bone_point(&pose, bone);
    let (pelvis, along) = (at(pelvis), at(head) - at(pelvis));
    let chest = along.cross(at(left) - at(right)).z;
    Some((along.y.atan2(along.x).to_degrees(), pelvis, chest))
}

/// The turn (degrees about the GLA root's up axis, as `Figure::pose_facing`
/// turns the body) that lays the first frame of the get-up `getup` where
/// the last frame of the death `lain` left the body's head, measured on the
/// skeleton both play on.
pub fn getup_turn(
    gla: &Gla,
    sequences: &BTreeMap<String, Sequence>,
    lain: Anim,
    getup: Anim,
) -> Option<f32> {
    let heading = |anim: Anim, last| {
        let sequence = sequences.get(anim.name())?;
        lying(gla, sequence, last).map(|(heading, ..)| heading)
    };
    let turn = heading(lain, true)? - heading(getup, false)?;
    Some((turn + 180.0).rem_euclid(360.0) - 180.0)
}

impl Rising {
    /// Getting up from lying in `lain` (the legs' animation shown).
    fn new(rig: &Rig, lain: Option<Anim>) -> Rising {
        let lain = lain.unwrap_or(Anim::BOTH_DEATH1);
        let getup = sim::combat::death::getup(lain);
        let length_ms = rig.sequences.get(getup.anim.name()).map_or(0.0, |s| {
            s.frame_count as f32 * 1000.0 / s.frames_per_second.unsigned_abs().max(1) as f32
        });
        Rising {
            anim: getup.anim,
            // Measured on the skeleton itself; a death the rig lacks keeps
            // the sim's table's turn, which is measured on the retargeted
            // KOTOR body, whose frame turns the other way.
            turn: getup_turn(rig.gla, &rig.sequences, lain, getup.anim).unwrap_or(-getup.turn),
            length_ms,
            blended: [false; 2],
            legs_from_ms: None,
        }
    }

    /// The turn left `since` ms into the get-up: all of it while the body
    /// still lies, eased out as it stands.
    fn turn_at(&self, since: f32) -> f32 {
        let held = self.length_ms * RISE_TURN_HELD;
        let f = ((since - held) / (self.length_ms - held).max(1.0)).clamp(0.0, 1.0);
        self.turn * (1.0 - f * f * (3.0 - 2.0 * f))
    }
}

/// What one part plays: its animation, and the sequence played in its
/// place (`Shown::plays`).
type PartPlays = (Anim, sim::skeleton::Played);

/// One character's animation state, carried from frame to frame.
pub struct Figure {
    g2: G2<'static>,
    bones: Bones,
    look: angles::State,
    /// What the legs and torso were last set to (animation, flip).
    legs: Option<(Anim, bool)>,
    torso: Option<(Anim, bool)>,
    /// What the legs and torso were last set to from the sim's skeleton.
    adopted: [Option<sim::combat::saber::blades::Shown>; 2],
    /// The last step's angles: the legs' yaw, the spine's and neck's turns.
    pub turned: Option<angles::Pose>,
    /// When it died (sim ms).
    died: Option<u64>,
    /// Whether its snapshots have been followed yet. A body not yet followed
    /// when it is found dead died before the view first saw it.
    followed: bool,
    /// When it last got up from lying dead or downed (sim ms), and how
    /// (`Rising`, a body on JA's animations).
    rose: Option<u64>,
    rising: Option<Rising>,
    ready: &'static str,
    /// Its melee wield number (`Sheet::wield_number`): the ready stance it
    /// holds after a scene's round (`acting`).
    wield: u8,
    /// Whether it wears TSL's wrist launcher (`acting::Throw`).
    launcher: bool,
    /// Whether it has a weapon equipped in either hand (`weapons::armed`).
    equipped: bool,
    /// The legs' and the torso's whole frame at the last step, and the
    /// animation it was in.
    frames: [Option<(i32, Anim)>; 2],
    /// The keyed frame the legs and the torso each sounded last.
    sounded: [asset_ja::events::Sounded; 2],
    /// The torso as the snapshots last had it (animation, flip), and when
    /// each hand's gun last fired (sim ms; right, left) and fired before
    /// that: its kick runs from there.
    shown_torso: Option<(Anim, bool)>,
    shot_ms: [Option<f64>; 2],
    shot_before_ms: [Option<f64>; 2],
    /// Its run of shots, which leans the chest during sustained fire
    /// (`recoil::Streak`).
    streak: crate::recoil::Streak,
    /// Its gun put away, the torso playing the legs' part (`follow`).
    away: bool,
    /// How far each hand's gun is held up and aimed (`gun_aim`), as of
    /// when.
    aim: [f32; 2],
    /// Its holstered guns drawn or put away (`draw_anim`).
    drawing: crate::draw_anim::Drawing,
    aim_ms: Option<f64>,
    /// It stands in one of JA's held stances (`stance::holding`), and how
    /// much it breathes in it (`stance`).
    holding: bool,
    /// Whether each part (legs, torso) last played another sequence than
    /// its animation's own (`Shown::plays`): a stance set's or JA's parry.
    duel_shown: [bool; 2],
    /// Its torso's last change to or from another sequence than its
    /// animation's own: the share of such a sequence's pose in its pose
    /// then (`duel_share`), when that was (sim ms), and how long its pose
    /// blends from the one before (ms).
    duel_from: (f32, f32, f32),
    /// What its legs and torso play as last steered from the sim's
    /// skeleton (animation, `Shown::plays`), and the names of those
    /// sequences: the animation's own, or the one a stance set or a held
    /// block plays in its place (`StanceSets::name_of`). None while it is
    /// not steered that way.
    shown: Option<([PartPlays; 2], [String; 2])>,
    /// A part played another sequence than its animation's own at the last
    /// step.
    swapped: bool,
    /// Under the duel rules, its torso shows a held block's guard while its
    /// legs walk or run.
    guard_walks: bool,
    /// Under the duel rules, its torso plays another sequence than its
    /// animation's own: a stance set's stance, walk, guard or parry, or JA's
    /// parry for a held block. Its hands then reach for the sim's blades as
    /// in JA's held stances (`reach_share`), and the left hand stays on a
    /// lone hilt that the right hand carries there (`Reach::carries`). A
    /// double-bladed hilt never counts as guarding.
    guarding: bool,
    /// Its saber move cuts (an attack, a special, a parry; not a start, a
    /// return, a transition or none): only then do the hands reach for the
    /// sim's blades in full (`reach_share`).
    cuts: bool,
    /// It is in a saber lock: the hands then reach for the sim's blades in
    /// full too, as far as `LOCK_REACH`.
    locked: bool,
    breath: stance::Breath,
    /// What its poses ease over time: its left hand on a lone hilt and a
    /// woman's hips (`Retargeter::pose`).
    eased: Eased,
    /// How much its gripping hands reach for the sim's blades
    /// (`reach_share`), as of when (sim ms).
    reach: (f32, Option<f64>),
    /// How much its gripping hands turn the drawn blades the sim's way
    /// under the duel rules (`turn_share`), as of when (sim ms).
    turn: (f32, Option<f64>),
    /// How far its blade at rest is held up before its eyes (`place`), as
    /// of when.
    held: f32,
    held_ms: Option<f64>,
    /// Its hand grips a blade (a saber's or a melee weapon's) as posed.
    blade: bool,
    /// Its weapons readied for a fight as last steered (`weapons::armed`,
    /// alive), not only carried.
    readied: bool,
    /// The last of its deflections seen (its event's sequence), and the
    /// held blade's flick across the newest: since when (ms) and where
    /// (game space, metres).
    deflected: Option<u16>,
    flick: Option<(f64, Vec3)>,
    /// Its head's turn toward whom it looks at in a conversation (S7).
    head: Head,
    /// Its guns held up for a charge or an aim (`raise`): the shot's frame
    /// they are held at, since when (sim ms) they went up, and since when
    /// they came down from it.
    raised: Option<Raised>,
    /// The torso `raise` last showed while the guns were held up or being
    /// let down.
    raised_torso: Option<sim::combat::saber::blades::Shown>,
    /// How the torso's pose jumped when the guns' state last changed
    /// (`Faded`).
    faded: Option<Faded>,
    /// The bones the torso's animation plays on (`lower_lumbar` and the
    /// bones under it, and `Motion`): those the aim pose mixes.
    upper: Vec<usize>,
}

/// The torso's bones (`Figure::upper`, in order) as shown just before the
/// guns' state changed at `ms` (sim ms), and as the new state shows them
/// then. A change can come from [Aim], a charge, a shot or the sim's aim
/// level, in any order. When the two differ, the difference is added to the
/// torso and dies away over `RAISE_MS`, so that the gun never jumps. The
/// torso goes on moving as the new state moves it meanwhile.
#[derive(Debug, Clone)]
struct Faded {
    before: Vec<Local>,
    after: Vec<Local>,
    ms: u64,
}

/// How far (radians) one torso bone must jump on a change of the guns'
/// state for the jump to die away (`Faded`) instead of being shown.
const FADE_FROM: f32 = 0.005;

/// Guns held up at `frame` (the shot's last) since `up_ms`; let down at
/// `down_ms`.
///
/// - **A timed raise** (no `level`) shows the held frame, blended up to it
///   over `RAISE_MS` from the pose shown when it began (`rise_from`, or the
///   ready pose's frame `from`). A raise begun during a shot (`carried`)
///   shows the held frame with no blend, since the shot leaves the arms
///   there.
/// - **A raise at the sim's level** (`level`, eased) leaves the torso the
///   sim plays and mixes it toward the held frame by the level
///   (`Figure::aim_pose`). After a shot the sim's torso blends from the
///   shot's end, which is the held frame, so the arms stay out.
/// - **A shot** plays unmixed (`in_shot`), since it carries the arms out by
///   itself. It starts from the pose shown when it began (`shot_from`: when
///   the shot began, and its blend, or none for the sim's own).
/// - **While the guns are let down**, `lowered` is the ready pose's
///   playback they blend into.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Raised {
    frame: i32,
    from: i32,
    up_ms: u64,
    down_ms: Option<u64>,
    level: Option<f32>,
    in_shot: bool,
    carried: bool,
    rise_from: Option<sim::skeleton::Blend>,
    lowered: Option<sim::skeleton::Playback>,
    shot_from: Option<(u64, Option<sim::skeleton::Blend>)>,
}

/// A blend from frame `frame` over `RAISE_MS` from `start_ms`.
fn blend_from(frame: i32, start_ms: u64) -> sim::skeleton::Blend {
    sim::skeleton::Blend {
        start_ms: start_ms as i64,
        length_ms: RAISE_MS,
        frame: frame as f32,
        next: frame,
        under: None,
    }
}

/// A blend over `RAISE_MS` from what `playback` shows at `at_ms`, with the
/// blend it is in, if one is under way.
fn blend_from_shown(playback: &sim::skeleton::Playback, at_ms: u64) -> sim::skeleton::Blend {
    let shown = playback.frames(at_ms as i64);
    sim::skeleton::Blend {
        start_ms: at_ms as i64,
        length_ms: RAISE_MS,
        frame: shown.current as f32 + shown.lerp,
        next: shown.next,
        under: (playback.blending(at_ms as i64)).map(|(blend, amount)| sim::skeleton::Under {
            frame: blend.frame,
            next: blend.next,
            start: playback.frames(blend.start_ms),
            amount,
        }),
    }
}

/// The frame `blend` starts from, the frame it mixes toward and how far,
/// as `Raised::blend_toward_held` takes them. A blend it began in partway
/// (`under`) is left out.
fn base_of(blend: sim::skeleton::Blend) -> (f32, i32, sim::skeleton::Frames) {
    let start = sim::skeleton::Frames {
        current: blend.frame as i32,
        next: blend.next,
        lerp: blend.frame.fract(),
    };
    (blend.frame, blend.next, start)
}

impl Raised {
    /// Its state apart from the sim's level, which moves a little at each
    /// step: whether a level is followed is kept.
    fn mode(raised: Option<Raised>) -> Option<Raised> {
        raised.map(|raised| Raised {
            level: raised.level.map(|_| 1.0),
            ..raised
        })
    }

    /// How far up the guns are at `ms` (0 to 1): the level, or the timed
    /// raise's share of `RAISE_MS` since they went up or came down.
    fn share(&self, ms: f64) -> f32 {
        let timed = |since: u64| ((ms - since as f64) as f32 / RAISE_MS as f32).clamp(0.0, 1.0);
        match (self.level, self.down_ms) {
            (Some(level), _) => level,
            (None, Some(down_ms)) => 1.0 - timed(down_ms),
            (None, None) => timed(self.up_ms),
        }
    }

    /// When the guns' let-down began (sim ms), starting it at `now_ms` if
    /// it has not begun. Guns let down before they were fully up come down
    /// from how far up they are, as if the let-down had begun that much
    /// earlier.
    fn let_down(&mut self, now_ms: u64) -> u64 {
        let short = 1.0 - self.share(now_ms as f64);
        *(self.down_ms).get_or_insert(now_ms.saturating_sub((short * RAISE_MS as f32) as u64))
    }

    /// A blend over `RAISE_MS` from `at_ms` that starts from the pose
    /// shown when the torso is `share` (0 to 1) of the way from frame
    /// `base` (mixed toward `next`, as it stood at `start`) to the held
    /// frame.
    fn blend_toward_held(
        &self,
        (base, next, start): (f32, i32, sim::skeleton::Frames),
        share: f32,
        at_ms: u64,
    ) -> sim::skeleton::Blend {
        sim::skeleton::Blend {
            start_ms: at_ms as i64,
            length_ms: RAISE_MS,
            frame: base,
            next,
            under: Some(sim::skeleton::Under {
                frame: self.frame as f32,
                next: self.frame,
                start,
                amount: 1.0 - share,
            }),
        }
    }

    /// The blend a shot fired at `at_ms` starts with: from the pose shown
    /// then. `sim` is the blend the sim starts the shot with, from its own
    /// ready pose. None plays the shot as the sim blends it.
    fn shot_blend(
        &self,
        at_ms: u64,
        sim: Option<sim::skeleton::Blend>,
    ) -> Option<sim::skeleton::Blend> {
        let still = |frame: i32| {
            let frames = sim::skeleton::Frames {
                current: frame,
                next: frame,
                lerp: 0.0,
            };
            (frame as f32, frame, frames)
        };
        let (base, shown) = match (self.down_ms, self.lowered, self.level) {
            // Let down into the ready pose: the held frame partway blended
            // into it.
            (Some(down_ms), Some(lowered), _) => {
                let at = lowered.frames(at_ms as i64);
                let base = (at.current as f32 + at.lerp, at.next);
                let start = lowered.frames(down_ms as i64);
                ((base.0, base.1, start), self.share(at_ms as f64))
            }
            // Let down after a shot: the sim's own torso is shown.
            (Some(_), None, _) => return None,
            // At the level: the sim's ready pose mixed toward the held
            // frame.
            (None, _, Some(level)) => (sim.map_or(still(self.from), base_of), level),
            // A timed raise: the held frame, or blended up to it.
            (None, _, None) => {
                let base = (self.rise_from).map_or(still(self.from), base_of);
                let shown = if self.carried {
                    1.0
                } else {
                    self.share(at_ms as f64)
                };
                (base, shown)
            }
        };
        Some(if shown >= 1.0 {
            blend_from(self.frame, at_ms)
        } else {
            self.blend_toward_held(base, shown, at_ms)
        })
    }
}

/// How long the guns take to go up for a charge or an aim (ms): a draw's
/// blend.
pub(crate) const RAISE_MS: u32 = 150;

impl Figure {
    pub(crate) fn new(rig: &Rig, yaw: f32, ready: &'static str) -> Figure {
        let mut g2 = G2::new(rig.gla);
        g2.quirks = rig.quirks;
        let bones = Bones::of(&g2);
        // A bone is the torso's if it or a bone above it is the torso's
        // part bone.
        let count = rig.gla.bones.len();
        let torso = |bone: usize| {
            let mut at = Some(bone);
            for _ in 0..count {
                let Some(index) = at.filter(|&index| index < count) else {
                    return false;
                };
                if bones.torso == Some(index) || bones.motion == Some(index) {
                    return true;
                }
                at = usize::try_from(rig.gla.bones[index].parent).ok();
            }
            false
        };
        let upper = (0..count).filter(|&bone| torso(bone)).collect();
        Figure {
            bones,
            g2,
            look: angles::State::facing(yaw),
            legs: None,
            torso: None,
            adopted: [None; 2],
            turned: None,
            died: None,
            followed: false,
            rose: None,
            rising: None,
            ready,
            wield: 2,
            launcher: false,
            equipped: false,
            frames: [None; 2],
            sounded: Default::default(),
            shown_torso: None,
            shot_ms: [None; 2],
            shot_before_ms: [None; 2],
            streak: crate::recoil::Streak::default(),
            away: false,
            aim: [0.0; 2],
            drawing: crate::draw_anim::Drawing::default(),
            aim_ms: None,
            holding: false,
            duel_shown: [false; 2],
            duel_from: (0.0, 0.0, 0.0),
            shown: None,
            swapped: false,
            guard_walks: false,
            guarding: false,
            cuts: false,
            locked: false,
            breath: stance::Breath::default(),
            eased: Eased::default(),
            reach: (1.0, None),
            turn: (1.0, None),
            held: 0.0,
            held_ms: None,
            blade: false,
            readied: false,
            deflected: None,
            flick: None,
            head: Head::default(),
            raised: None,
            raised_torso: None,
            faded: None,
            upper,
        }
    }

    /// The torso the sim's skeleton plays (`torso`), drawn with its guns held
    /// up as `lift` says (`aim_pose::lift`): the gun's ready pose shown as
    /// the last frame of its shot (`BOTH_ATTACK2`, `BOTH_ATTACK3`), the arms
    /// out where a shot leaves them. A timed raise blends up to that frame
    /// over `RAISE_MS`. A raise at the sim's level keeps the sim's torso
    /// and the level (`Raised::level`), and `pose_facing` mixes the pose
    /// toward the held frame by it. A shot plays from the pose shown when
    /// it began and leaves the arms out; the shot that raised the guns
    /// plays as the sim blends it.
    fn raise(
        &mut self,
        sequences: &BTreeMap<String, Sequence>,
        torso: sim::combat::saber::blades::Shown,
        lift: Lift,
        now_ms: u64,
    ) -> sim::combat::saber::blades::Shown {
        use sim::skeleton::Playback;
        // The gun's shot, its last frame and the ready pose's frame.
        let gun = match torso.anim {
            Anim::TORSO_WEAPONREADY2 | Anim::BOTH_ATTACK2 => {
                Some((Anim::BOTH_ATTACK2, Anim::TORSO_WEAPONREADY2))
            }
            Anim::TORSO_WEAPONREADY3 | Anim::BOTH_ATTACK3 => {
                Some((Anim::BOTH_ATTACK3, Anim::TORSO_WEAPONREADY3))
            }
            _ => None,
        };
        let frames = gun.and_then(|(shot, ready)| {
            let (held, ready) = (sequences.get(shot.name())?, sequences.get(ready.name())?);
            Some((
                shot,
                held.first_frame + held.frame_count - 1,
                ready.first_frame,
            ))
        });
        let shooting = matches!(torso.anim, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3);
        let held = lift != Lift::Down;
        let start_ms = torso.playback.start_ms as u64;
        if held && let Some((shot, frame, ready)) = frames {
            let mut raised = match self.raised {
                Some(raised) if raised.down_ms.is_none() => raised,
                was => {
                    let (up_ms, shot_from, rise_from) = match was {
                        // Raised again while being let down: the raise
                        // carries on from how far up the guns still are
                        // now, also during a shot. A shot fired then plays
                        // from the pose shown. Where the sim's own torso
                        // shows (after a shot), it rises from that.
                        Some(was) => {
                            let up = (was.share(now_ms as f64) * RAISE_MS as f32) as u64;
                            let shot_from = shooting.then(|| {
                                (was.shot_from.filter(|&(at, _)| at == start_ms)).unwrap_or_else(
                                    || (start_ms, was.shot_blend(start_ms, torso.playback.blend)),
                                )
                            });
                            let rise_from = (!shooting && was.lowered.is_none())
                                .then(|| blend_from_shown(&torso.playback, now_ms));
                            (now_ms.saturating_sub(up), shot_from, rise_from)
                        }
                        // Another character's guns first raised by its shot
                        // go up as the shot begins, so that the shot is the
                        // one that raised them and plays as the sim blends
                        // it.
                        None if shooting && lift == Lift::AfterShot => {
                            (start_ms.min(now_ms), None, None)
                        }
                        // Raised from the pose the sim's torso shows, which
                        // may still be blending out of a shot.
                        None => (
                            now_ms,
                            None,
                            Some(blend_from_shown(&torso.playback, now_ms)),
                        ),
                    };
                    Raised {
                        frame,
                        from: ready,
                        up_ms,
                        down_ms: None,
                        level: None,
                        in_shot: false,
                        carried: shooting,
                        rise_from,
                        lowered: None,
                        shot_from,
                    }
                }
            };
            // The sim's level takes over from a timed raise once it is as
            // high. Going from the level to a timed raise, the clock goes
            // on from how high the level had the guns, and from the sim's
            // torso the level mixed toward the held frame. During a shot
            // the shot leaves the arms at the held frame.
            let level = match lift {
                Lift::Level(level)
                    if raised.level.is_some() || level >= raised.share(now_ms as f64) =>
                {
                    Some(level)
                }
                _ => None,
            };
            if let (None, Some(was)) = (level, raised.level) {
                raised.up_ms = now_ms.saturating_sub((was * RAISE_MS as f32) as u64);
                raised.carried = shooting;
                raised.rise_from = (!shooting).then(|| sim::skeleton::Blend {
                    start_ms: raised.up_ms as i64,
                    ..blend_from_shown(&torso.playback, now_ms)
                });
            }
            raised.level = level;
            if !shooting {
                raised.in_shot = false;
                self.raised = Some(raised);
                if level.is_some() {
                    return torso;
                }
                let blend = match (raised.carried, raised.rise_from) {
                    (true, _) => None,
                    (false, Some(rise_from)) => Some(rise_from),
                    (false, None) => Some(blend_from(raised.from, raised.up_ms)),
                };
                return sim::combat::saber::blades::Shown {
                    anim: shot,
                    flip: torso.flip,
                    playback: Playback {
                        start_frame: frame,
                        end_frame: frame + 1,
                        speed: 0.0,
                        start_ms: raised.up_ms as i64,
                        looping: false,
                        blend,
                    },
                    plays: sim::skeleton::Played::OWN,
                };
            }
            self.raised = Some(raised);
        }
        let Some(mut raised) = self.raised else {
            return torso;
        };
        let mut torso = torso;
        raised.in_shot = shooting;
        if shooting {
            // The shot that raised them plays as the sim blends it. Any
            // other (let go, a pair's next, a sniper's) plays from the
            // pose shown when it began, and leaves the arms out.
            let blend = match raised.shot_from {
                Some((at, blend)) if at == start_ms => blend,
                _ => {
                    let blend = (start_ms > raised.up_ms)
                        .then(|| raised.shot_blend(start_ms, torso.playback.blend))
                        .flatten();
                    raised.shot_from = Some((start_ms, blend));
                    blend
                }
            };
            if let Some(blend) = blend {
                torso.playback.blend = Some(blend);
            }
            if !held {
                raised.let_down(now_ms);
            }
            self.raised = Some(raised);
            return torso;
        }
        if held {
            // Held up over a torso that is not the gun's.
            self.raised = Some(raised);
            torso.playback.blend = Some(blend_from(raised.frame, start_ms));
            return torso;
        }
        // Lowered at the sim's level, the guns are down already.
        if raised.level.is_some() {
            self.raised = None;
            return torso;
        }
        // Let down with no shot (a charge let go of empty-handed, another
        // character's last shot long enough ago): the ready pose blends
        // from the held guns; then it is the sim's again. An animation begun
        // since the let-down (the one after a shot let go) blends as the
        // sim has it from the shot's end, while the corrections go down.
        let down_ms = raised.let_down(now_ms);
        if now_ms > down_ms + u64::from(RAISE_MS) {
            self.raised = None;
            return torso;
        }
        if start_ms >= down_ms {
            raised.lowered = None;
            self.raised = Some(raised);
            return torso;
        }
        raised.lowered = Some(torso.playback);
        self.raised = Some(raised);
        torso.playback.blend = Some(blend_from(raised.frame, down_ms));
        torso
    }

    /// The keyframed sounds its legs and torso passed since the last step
    /// (`asset_ja::events::Passage`), at `ms`.
    fn keyframed<'e>(&mut self, rig: &Rig, ms: f64, events: &'e Events, out: &mut Vec<&'e Keyed>) {
        let parts = [
            (self.bones.root, self.legs, &events.legs),
            (self.bones.torso, self.torso, &events.torso),
        ];
        for (part, (bone, playing, keyed)) in parts.into_iter().enumerate() {
            let (Some(bone), Some((anim, _))) = (bone, playing) else {
                continue;
            };
            let Some(frame) = self.g2.frame_at(bone, ms as f32) else {
                continue;
            };
            let now = frame.floor() as i32;
            let Some((before, was)) = self.frames[part].replace((now, anim)) else {
                continue;
            };
            if before == now {
                continue;
            }
            let passage = Passage {
                before,
                now,
                same: (was == anim)
                    .then(|| rig.sequences.get(anim.name()))
                    .flatten(),
            };
            let sounded = &mut self.sounded[part];
            sounded.step(&passage);
            out.extend(keyed.iter().filter(|k| sounded.sounds(&passage, k.frame)));
        }
    }

    /// What its snapshot at `time_ms` says it plays. A living fighter's
    /// parts play as the sim's skeleton plays them (`adopt`), so its blades
    /// are drawn where they cut. Otherwise (and with `quirks`, the cgame's
    /// own way): an animation the legs or torso start (or restart, by a new
    /// flip) starts then, blended as the cgame blends it
    /// (`asset_ja::blend_ms`); legs on `model_root`, torso on `lower_lumbar`
    /// and `Motion`; a part switching to what the other part plays picks up
    /// its frame (`G2::set_part`). A gun put away (not `armed`) is not held
    /// ready: the torso plays what the legs play. A part the sim plays
    /// another sequence on (a stance set's, `Shown::plays`) is named by
    /// `stances`, the level's sets. Under the duel `rules` a held block's
    /// guard over walking legs turns the spine back (`guard_walks`). A body
    /// a seat acts through (`seat`) holds its guns up as the sim raises
    /// them; any other, after its shots (`aim_pose::lift`).
    pub(crate) fn follow(
        &mut self,
        rig: &Rig,
        (time_ms, rules, stances): (u64, SaberRules, Option<&sim::skeleton::StanceSets>),
        entity: &EntityState,
        (armed, seat): (bool, bool),
    ) {
        let Some(p) = entity.player else { return };
        // The sim plays a get-up (`sim::world::falls::get_up`); an arena's
        // respawn stands its body up at once, no get-up played.
        let getting_up = p.legs.anim.name().starts_with("BOTH_GETUP");
        let alive = entity.fighter.map(|f| f.alive());
        if alive == Some(false) && self.died.is_none() {
            // The snapshots do not say when a body died. A body already dead
            // when the view first sees it is taken to have died
            // `DEATH_PLAYS_MS` earlier, so that its death animation has
            // played to its end and the body lies down.
            self.died = Some(if self.followed {
                time_ms
            } else {
                time_ms.saturating_sub(DEATH_PLAYS_MS as u64)
            });
            self.rising = None;
        }
        self.followed = true;
        // Up again (a revive, the fight's end): its fall is over, the
        // get-up blending in from how it lay.
        if alive == Some(true) && self.died.take().is_some() && getting_up {
            self.rose = Some(time_ms);
            self.rising = Some(Rising::new(rig, self.legs.map(|(anim, _)| anim)));
        }
        // Over at the get-up's end, or before its legs took it up if the
        // sim no longer plays it: the body does not lie on.
        if let (Some(rose), Some(rising)) = (self.rose, self.rising)
            && (time_ms as f32 > rose as f32 + rising.length_ms
                || !rising.blended[0] && !getting_up)
        {
            self.rising = None;
        }
        let torso = (p.torso.anim, p.torso.flip);
        if self.shown_torso != Some(torso) {
            if matches!(torso.0, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3) {
                // The hands the shot left (`Shooting::hands`; one gun's is
                // the gun out's, the first).
                let hands = entity.fighter.map_or(1, |f| f.shooting.hands);
                for (hand, shot) in self.shot_ms.iter_mut().enumerate() {
                    if hands & 1 << hand != 0 || (hand == 0 && hands & 3 == 0) {
                        self.shot_before_ms[hand] = *shot;
                        *shot = Some(time_ms as f64);
                    }
                }
                self.streak.shot(time_ms as f64);
            }
            self.shown_torso = Some(torso);
        }
        // Holstering, its guns still in hand (`draw_anim`): the torso
        // keeps the ready pose it held, the arms carrying the guns down.
        let keep = !armed && self.drawing.pose().in_hand;
        let away = !armed
            && !keep
            && matches!(
                p.torso.anim,
                Anim::TORSO_WEAPONREADY2 | Anim::TORSO_WEAPONREADY3
            );
        self.holding = stance::holding(&rig.sequences, (p.legs.anim, p.torso.anim));
        self.cuts = entity.fighter.is_some_and(|f| cutting(&f.saber));
        self.locked = entity.fighter.is_some_and(|f| f.saber.lock.is_some());
        let posed = entity
            .fighter
            .filter(|f| f.alive() && !rig.quirks)
            .and_then(|f| Some([f.pose.legs?, f.pose.torso?]));
        if posed.is_none() {
            // Dead or drawn the cgame's way: no guns held up to come back to.
            self.raised = None;
            self.raised_torso = None;
            self.faded = None;
        }
        if let Some(mut parts) = posed {
            let last_shot = self.shot_ms.into_iter().flatten().reduce(f64::max);
            let lift = entity
                .fighter
                .filter(Fighter::alive)
                .map_or(Lift::Down, |f| {
                    aim_pose::lift(seat, &f.shooting, last_shot, time_ms as f64)
                });
            let (was, was_torso) = (self.raised, self.raised_torso);
            parts[1] = self.raise(&rig.sequences, parts[1], lift, time_ms);
            self.raised_torso = self.raised.map(|_| parts[1]);
            // The guns' state changed: the pose shown just before, from the
            // torso and the raise as they were.
            let before = ((was.is_some() || self.raised.is_some())
                && (Raised::mode(was) != Raised::mode(self.raised)
                    || was_torso != self.raised_torso))
                .then(|| {
                    let now = std::mem::replace(&mut self.raised, was);
                    let pose = self.upper_pose(time_ms);
                    self.raised = now;
                    pose
                });
            let duel = parts.map(|part| part.plays != sim::skeleton::Played::OWN);
            self.swapped = duel.contains(&true);
            // A held block's guard (or JA's parry) on the torso over legs
            // that walk or run their own sequence.
            self.guard_walks = rules.duel()
                && sim::skeleton::stance::moves(parts[0].anim)
                && duel[1]
                && parts[0].plays != parts[1].plays;
            self.guarding = rules.duel() && duel[1];
            let played = parts.map(|part| (part.anim, part.plays));
            if self.shown.as_ref().map(|(was, _)| was) != Some(&played) {
                let name = |(anim, plays): PartPlays| {
                    stances
                        .filter(|_| plays != sim::skeleton::Played::OWN)
                        .and_then(|sets| sets.name_of(plays))
                        .unwrap_or_else(|| anim.name().to_string())
                };
                self.shown = Some((played, played.map(name)));
            }
            if self.swapped {
                // Its own stance moves; JA's held pose does not breathe.
                self.holding = false;
            }
            // Up again, the sim's skeleton may show a step of its own before
            // the get-up: the body lies on until it takes the get-up up.
            let lies = self
                .rising
                .is_some_and(|rising| !rising.blended[0] && parts[0].anim != rising.anim);
            if !lies {
                self.adopt(parts, (away, keep), duel, time_ms as f32);
            }
            if let Some(before) = before {
                self.fade_from(before, time_ms);
            }
            if (self.faded.as_ref()).is_some_and(|faded| time_ms >= faded.ms + u64::from(RAISE_MS))
            {
                self.faded = None;
            }
            return;
        }
        self.adopted = [None; 2];
        self.shown = None;
        self.swapped = false;
        self.guard_walks = false;
        self.guarding = false;
        self.away = away;
        let torso = if away {
            (p.legs.anim, p.legs.flip)
        } else if keep {
            self.torso.unwrap_or(torso)
        } else {
            torso
        };
        let after = (p.legs.anim.name(), torso.0.name());
        let before = (
            self.legs.map_or("", |(anim, _)| anim.name()),
            self.torso.map_or("", |(anim, _)| anim.name()),
        );
        let changed = [
            (Part::Legs, (p.legs.anim, p.legs.flip), self.legs),
            (Part::Torso, torso, self.torso),
        ];
        for (part, now, was) in changed {
            if was == Some(now) {
                continue;
            }
            let rises = self.rising.is_some_and(|rising| rising.anim == now.0) && !rig.quirks;
            let blend = match was {
                Some(_) if rises => RISE_BLEND_MS,
                Some((old, _)) => asset_ja::blend_ms(Some(old.name()), now.0.name(), rig.quirks),
                None => 0.0,
            };
            if let Some(sequence) = rig.sequences.get(now.0.name()) {
                // A dead body's clip is timed from its death. A body found
                // dead is therefore drawn at the clip's end, lying down.
                let at_ms = match alive {
                    Some(false) => self.died.unwrap_or(time_ms),
                    _ => time_ms,
                };
                let set = PartSet {
                    part,
                    sequence,
                    at_ms: at_ms as f32,
                    blends: (blend, blend),
                    // The split kick plays slower than JA's (the sim holds
                    // it as much longer).
                    speed_scale: sim::motion::clip_speed(now.0),
                    before,
                    after,
                };
                self.g2.set_part(&set, self.bones);
            }
            match part {
                Part::Legs => self.legs = Some(now),
                _ => self.torso = Some(now),
            }
        }
    }

    /// The sim skeleton's parts (legs, torso), each played as the sim set
    /// it, when it changed: the legs on `model_root`, the torso on
    /// `lower_lumbar` and `Motion`. With its gun `away` the torso plays the
    /// legs' part; going to or from that, it blends from the pose shown at
    /// `now_ms` (`DRAW_BLEND_MS`, `HOLSTER_BLEND_MS`). Kept (`keep`: guns
    /// still in hand on their way to the holsters), the torso holds what it
    /// played. `duel` says which parts play another sequence than their
    /// animation's own (`Shown::plays`).
    fn adopt(
        &mut self,
        parts: [sim::combat::saber::blades::Shown; 2],
        (away, keep): (bool, bool),
        duel: [bool; 2],
        now_ms: f32,
    ) {
        let bones = [
            vec![self.bones.root],
            vec![self.bones.torso, self.bones.motion],
        ];
        let (parts, duel) = if away {
            ([parts[0], parts[0]], [duel[0], duel[0]])
        } else {
            (parts, duel)
        };
        let switched = away != self.away;
        self.away = away;
        // A part taking up the get-up blends in from the pose it lay in.
        let mut rising = self.rising;
        let duel_share = self.duel_share(f64::from(now_ms));
        for (part, ((shown, was), bones)) in parts
            .into_iter()
            .zip(&mut self.adopted)
            .zip(bones)
            .enumerate()
        {
            if *was == Some(shown) || (keep && part == 1) {
                continue;
            }
            *was = Some(shown);
            // The share of a stance's pose blends over the sim's own blend
            // (`duel_share`).
            if part == 1 && self.duel_shown[part] != duel[part] {
                let blend_ms = shown
                    .playback
                    .blend
                    .map_or(0.0, |blend| blend.length_ms as f32);
                self.duel_from = (duel_share, now_ms, blend_ms);
            }
            self.duel_shown[part] = duel[part];
            let p = &shown.playback;
            let timed = asset_ja::g2::Timed {
                start_frame: p.start_frame,
                end_frame: p.end_frame,
                speed: p.speed,
                start_ms: p.start_ms as f32,
                looping: p.looping,
                blend: p.blend.map(|b| asset_ja::g2::BlendFrom {
                    start_ms: b.start_ms as f32,
                    time_ms: b.length_ms as f32,
                    frame: b.frame,
                    next: b.next,
                    under: b.under.map(|u| asset_ja::g2::BlendUnder {
                        frame: u.frame,
                        next: u.next,
                        start: (u.start.current, u.start.next, u.start.lerp),
                        amount: u.amount,
                    }),
                }),
            };
            let rises = match rising.as_mut() {
                Some(rising) if rising.anim == shown.anim && !rising.blended[part] => {
                    rising.blended[part] = true;
                    if part == 0 {
                        rising.legs_from_ms = Some(now_ms);
                    }
                    true
                }
                _ => false,
            };
            let blend_ms = if rises {
                RISE_BLEND_MS
            } else if away {
                HOLSTER_BLEND_MS
            } else {
                DRAW_BLEND_MS
            };
            // The legs' root blends from the lying pose turned back by the
            // get-up's turn, which `pose_facing` turns the whole body by.
            let unturn = rising
                .filter(|_| rises && part == 0)
                .map(|rising| Quat::from_rotation_z(-rising.turn.to_radians()));
            let g2 = &mut self.g2;
            for &bone in bones.iter().flatten() {
                if let Some(unturn) = unturn {
                    g2.adopt_blending_turned(bone, &timed, now_ms, blend_ms, unturn);
                } else if rises || (part == 1 && switched) {
                    g2.adopt_blending(bone, &timed, now_ms, blend_ms);
                } else {
                    g2.adopt(bone, &timed);
                }
            }
        }
        self.rising = rising;
        let [legs, torso] = parts.map(|part| Some((part.anim, part.flip)));
        self.legs = legs;
        if !keep {
            self.torso = torso;
        }
    }

    /// The angles at `ms`, `frame_ms` after the last step (which advances
    /// their swinging), as JA's client turns them each rendered frame.
    /// `motion` reads the torso's `Motion` bolt, a pose's work.
    fn steer(&mut self, rig: &Rig, at: &Steered, ms: f64, frame_ms: f32, motion: bool) {
        let (Some((legs, _)), Some((torso, _))) = (self.legs, self.torso) else {
            return;
        };
        // A held block's guard over walking legs counts as the torso's own
        // sequence, so the spine is corrected as JA's client corrects it
        // while the legs and torso play different sequences. The guard's
        // `Motion` bone does not follow the walk, so the correction is how
        // far the walk turns the hips (the line between the hip joints)
        // from where the guard's own frame has them. The chest then stands
        // to the hips as the guard has it standing.
        let names = match &self.shown {
            Some((_, [legs, torso])) if self.guard_walks => [legs.as_str(), torso.as_str()],
            _ => [legs.name(), torso.name()],
        };
        let hips = self
            .guard_walks
            .then(|| Some((self.g2.bone("rfemurYZ")?, self.g2.bone("lfemurYZ")?)))
            .flatten();
        let motion = match (self.bones.motion, hips) {
            (_, Some(hips)) if motion => {
                let pose = self.g2.pose(ms as f32);
                let guard = (self.bones.torso)
                    .and_then(|torso| self.g2.frame_at(torso, ms as f32))
                    .map_or(0.0, |frame| {
                        self.g2.line_yaw(&self.g2.frame_pose(frame), hips)
                    });
                let walk = self.g2.line_yaw(&pose, hips);
                [0.0, angles::wrap_180(walk - guard), 0.0]
            }
            (Some(bone), _) if motion && names[0] != names[1] => {
                self.g2.bolt_angles_at(ms as f32, bone)
            }
            _ => [0.0; 3],
        };
        let moment = angles::Moment {
            steer: angles::Steer {
                view: at.view,
                velocity: at.state.velocity,
                move_dir: at.state.push_dir,
                on_ground: at.state.ground.is_some(),
                look: None,
            },
            legs: names[0],
            torso: names[1],
            // With the gun away the torso stands as the legs do, and is
            // ready so.
            ready: if self.away { torso.name() } else { self.ready },
            dead: !at.alive,
            quirks: rig.quirks,
            motion,
        };
        self.turned = Some(angles::player_angles(
            &mut self.look,
            &moment,
            ms as f32,
            frame_ms,
        ));
    }

    /// How much its gripping hands move to put the drawn blades where the
    /// sim's are (`Reach::share`) at `ms`: all while its saber move cuts
    /// (`cuts`) or it is in a saber lock (`locked`), eased over
    /// `REACH_EASE_S`; `STANCE_REACH` while it stands in one of JA's held
    /// stances (`holding`) or, under the duel rules, its torso plays a
    /// stance set's sequence or a parry (`guarding`), eased in over
    /// `STANCE_EASE_S`;
    /// none otherwise (a start, a return, a transition, a flip, a run:
    /// nothing is cut, and JA's arm is the truth). How far the blades turn
    /// the sim's way is `turn_share`'s.
    fn reach_share(&mut self, ms: f64) -> f32 {
        let (target, ease) = match (self.cuts || self.locked, self.holding || self.guarding) {
            (true, _) => (1.0, REACH_EASE_S),
            (false, true) => (STANCE_REACH, STANCE_EASE_S),
            (false, false) => (0.0, REACH_EASE_S),
        };
        eased(&mut self.reach, (target, ease), ms)
    }

    /// How much of its torso's pose at `ms` is a duel stance's (0 to 1).
    /// The pose blends to or from a duel stance's sequence at an even pace
    /// (`adopt`), and the share follows that blend from what it was when
    /// the blend began (`duel_from`). Under the duel rules a hilt turns in
    /// the fist by this share (`Retargeter::pose`), because a Movie Duels
    /// stance may hold the saber in a reverse grip. JA's own sequences
    /// twirl the hilt in the hand too, and there the hand keeps turning
    /// with the hilt.
    fn duel_share(&self, ms: f64) -> f32 {
        let (was, from, length) = self.duel_from;
        let wanted = if self.duel_shown[1] { 1.0 } else { 0.0 };
        let blended = if length > 0.0 {
            ((ms as f32 - from) / length).clamp(0.0, 1.0)
        } else {
            1.0
        };
        was + (wanted - was) * blended
    }

    /// How much its gripping hands turn the drawn blades the sim's way
    /// (`Reach::turn`) at `ms` under the duel rules. During an attack's
    /// lead-in (a start or a transition, `lead_ms` of it left) the rest of
    /// the turn is spread evenly over the rest of the lead-in, so it is
    /// whole as the cut begins: the drawn blade is where the sim's cuts from
    /// the cut's first frame. In contact with no lead-in (a block, a bolt
    /// deflection, a saber lock, an attack with no start) they turn all the
    /// way, eased in over `CONTACT_EASE_S`. While the torso plays a stance
    /// set's sequence or a parry (`guarding`) they turn all the way too,
    /// eased in over `STANCE_EASE_S`. Otherwise the turn eases away
    /// over `REACH_EASE_S` and the drawn pose keeps its own turn.
    fn turn_share(&mut self, (contact, lead_ms): (bool, Option<f32>), ms: f64) -> f32 {
        if let Some(left) = lead_ms {
            let (turn, was) = self.turn;
            let seconds = was.map_or(1.0, |was| ((ms - was) / 1000.0).max(0.0) as f32);
            let left = left / 1000.0;
            let turn = if left <= seconds {
                1.0
            } else {
                turn + (1.0 - turn) * (seconds / left)
            };
            self.turn = (turn, Some(ms));
            return turn;
        }
        let (target, ease) = if contact {
            (1.0, CONTACT_EASE_S)
        } else if self.guarding {
            (1.0, STANCE_EASE_S)
        } else {
            (0.0, REACH_EASE_S)
        };
        eased(&mut self.turn, (target, ease), ms)
    }

    /// Its hand grips a blade as last posed.
    pub fn blade(&self) -> bool {
        self.blade
    }

    /// The names of the sequences its legs and torso play, as last steered
    /// from the sim's skeleton: a part that plays another sequence than its
    /// animation's own is named by `StanceSets::name_of`. None while it is
    /// not steered that way (dead, or drawn the cgame's way).
    pub fn shown_parts(&self) -> Option<[&str; 2]> {
        self.shown
            .as_ref()
            .map(|(_, [legs, torso])| [legs.as_str(), torso.as_str()])
    }

    /// A part played another sequence than its animation's own at the last
    /// step (`Shown::plays`).
    pub fn swapped(&self) -> bool {
        self.swapped
    }

    /// Its saber move cuts at the last step (`reach_share`).
    pub fn cuts(&self) -> bool {
        self.cuts
    }

    /// Where each of JA's bones `names` (any case) is in the JA pose at
    /// `ms` (model space, metres), for the bones the skeleton has.
    pub fn ja_points<'n>(&self, ms: f64, names: &[&'n str]) -> Vec<(&'n str, Vec3)> {
        let pose = self.pose(ms);
        names
            .iter()
            .filter_map(|&name| Some((name, self.g2.bone_point(&pose, self.g2.bone(name)?))))
            .collect()
    }

    /// When its snapshots first said it was dead (sim ms).
    pub fn died(&self) -> Option<u64> {
        self.died
    }

    /// The legs' yaw (degrees): the model faces it.
    pub fn yaw(&self) -> Option<f32> {
        self.turned.as_ref().map(|turned| turned.legs_yaw)
    }

    /// Where `bolt` (JA's model space at its base pose) is in the JA pose
    /// at `ms` (model space, metres).
    pub fn bolt(&self, ms: f64, bolt: &asset_ja::Bolt) -> Mat4 {
        Self::bolted(&self.g2, &self.pose(ms), bolt)
    }

    fn bolted(g2: &G2, pose: &[Local], bolt: &asset_ja::Bolt) -> Mat4 {
        let (rotation, translation) = g2.bone_final(pose, bolt.bone);
        Mat4::from_rotation_translation(rotation, translation) * crate::scene::to_mat4(&bolt.matrix)
    }

    /// The JA pose at `ms`, turned as the cgame turns it: the spine and
    /// neck by their angles, the whole model to the legs' yaw.
    pub fn pose(&self, ms: f64) -> Vec<Local> {
        self.pose_facing(ms, true)
    }

    /// `pose`, the whole model turned to the legs' yaw only if `facing`.
    fn pose_facing(&self, ms: f64, facing: bool) -> Vec<Local> {
        let g2 = &self.g2;
        // Drawn before the get-up's blend begins (between steps), the body
        // shows where the blend starts, the lying pose: the get-up's own
        // first frame, unblended, would take the turn below as well.
        let from = self.rising.and_then(|rising| rising.legs_from_ms);
        let mut pose = g2.pose(from.map_or(ms as f32, |from| from.max(ms as f32)));
        self.aim_pose(&mut pose, ms);
        if let Some(turned) = &self.turned {
            for &(bone, angles) in &turned.bones {
                if let Some(bone) = g2.bone(bone) {
                    g2.turn(&mut pose, bone, angles);
                }
            }
            if let Some(root) = self.bones.root.filter(|_| facing) {
                // A get-up turned to start where the death left the body,
                // once its legs have taken it up (`adopt`).
                let turn = self
                    .rising
                    .filter(|rising| rising.legs_from_ms.is_some())
                    .zip(self.rose)
                    .map_or(0.0, |(rising, rose)| {
                        rising.turn_at((ms - rose as f64) as f32)
                    });
                let yaw = Quat::from_rotation_z((turned.legs_yaw + turn).to_radians());
                pose[root] = (yaw * pose[root].0, yaw * pose[root].1);
            }
        }
        pose
    }

    /// Holds a gun of KOTOR wield number `wield` (`stance::wield`), as the
    /// cast sets it from the character's sheet.
    #[cfg(feature = "probes")]
    pub(crate) fn hold_gun(&mut self, wield: u8) {
        self.breath.wield = wield;
    }

    /// The largest angle (radians) by which one torso bone jumped at the
    /// guns' last change of state, the jump that dies away (`Faded`), and
    /// that bone (its number in the skeleton). Zero, on bone 0, when no jump
    /// has needed to die away.
    #[cfg(feature = "probes")]
    pub(crate) fn faded_jump(&self) -> (f32, usize) {
        self.faded.as_ref().map_or((0.0, 0), |faded| {
            (self.upper.iter().zip(faded.before.iter().zip(&faded.after)))
                .map(|(&bone, (before, after))| (before.0.angle_between(after.0), bone))
                .fold(
                    (0.0, 0),
                    |most, jump| if jump.0 > most.0 { jump } else { most },
                )
        })
    }

    /// How far its guns are raised at `ms`, from 0 at the hip to 1 before
    /// the eyes: the share by which `aim_pose` adds the wield's
    /// corrections. A player's follows the sim's raise, and another
    /// character's is timed (`Raised::share`).
    pub(crate) fn raise_share(&self, ms: f64) -> f32 {
        self.raised.map_or(0.0, |raised| raised.share(ms))
    }

    /// Whether it stands in one of JA's held stances or fires from one, so
    /// that it breathes while it stands still (`stance::holding`).
    #[cfg(feature = "probes")]
    pub(crate) fn holding(&self) -> bool {
        self.holding
    }

    /// How far (radians) its head leans toward the gun side at `ms`: the
    /// wield's lean (`aim_pose::head_tilt_deg`) by how far up the guns are,
    /// so it comes in with the raise and goes with the let-down.
    fn head_tilt(&self, ms: f64) -> f32 {
        let share = self.raised.map_or(0.0, |raised| raised.share(ms));
        let wield = aim_pose::wield_of(self.breath.wield);
        (aim_pose::head_tilt_deg(wield) * share).to_radians()
    }

    /// Its guns held up in `pose` at `ms` (`raise`): held at the sim's
    /// level, the torso's bones mixed from the sim's pose toward the held
    /// frame by it, except while a shot plays; then the wield's corrections
    /// (`aim_pose::corrections`) added by how far up the guns are. Last,
    /// what is left of the torso's jump at the guns' last change of state
    /// is added (`Faded`).
    fn aim_pose(&self, pose: &mut [Local], ms: f64) {
        let g2 = &self.g2;
        if let Some(raised) = self.raised {
            if let Some(level) = raised.level.filter(|_| !raised.in_shot) {
                let held = g2.frame_pose(raised.frame as f32);
                for &bone in &self.upper {
                    pose[bone] = g2.mix(bone, pose[bone], held[bone], level.min(1.0));
                }
            }
            let share = raised.share(ms);
            if share > 0.0 {
                let wield = aim_pose::wield_of(self.breath.wield);
                for &(name, angles) in aim_pose::corrections(wield) {
                    if let Some(bone) = g2.bone(name) {
                        g2.turn(pose, bone, angles.map(|angle| angle * share));
                    }
                }
            }
        }
        if let Some(faded) = &self.faded {
            let t = ((ms - faded.ms as f64) / f64::from(RAISE_MS)).clamp(0.0, 1.0) as f32;
            // What is left of the jump, by smoothstep: no speed at either end.
            let left = 1.0 - t * t * (3.0 - 2.0 * t);
            let jumps = faded.before.iter().zip(&faded.after);
            for (&bone, (&before, &after)) in self.upper.iter().zip(jumps) {
                pose[bone] = g2.offset(bone, pose[bone], (before, after), left);
            }
        }
    }

    /// After the guns' state changed at `ms`: if the torso now shows a
    /// pose apart from `before`, the one shown just before, the jump dies
    /// away from then (`Faded`). A jump still dying away is in `before`.
    fn fade_from(&mut self, before: Vec<Local>, ms: u64) {
        let fading = self.faded.take();
        let after = self.upper_pose(ms);
        let jumps =
            (before.iter().zip(&after)).any(|(was, now)| was.0.angle_between(now.0) > FADE_FROM);
        self.faded = if jumps {
            Some(Faded { before, after, ms })
        } else {
            fading
        };
    }

    /// The torso's bones (`upper`, in order) as `aim_pose` leaves them at
    /// `ms`, before they are turned to the look.
    fn upper_pose(&self, ms: u64) -> Vec<Local> {
        let mut pose = self.g2.pose(ms as f32);
        self.aim_pose(&mut pose, ms as f64);
        self.upper.iter().map(|&bone| pose[bone]).collect()
    }
}

/// The KOTOR clips a creature plays, bound to its nodes (by `PAUSE`,
/// `WALK`, `RUN`, `DIE`, `DEAD`, `GETUP`), the one it showed last and how
/// far in, and the one it left, fading out (where it stopped, and when,
/// sim ms); its clips for scripts' animations and what it shows of them
/// (`acting`) and of its grenade throws; the metres its walk and run cycles cover and how far into
/// each it is, as of when (sim ms); its nodes' parents and rest
/// (`rest_of`); how it leans with the floor, if it does (`ground_lean`);
/// whether it is a creature model (`CreatureClips::creature`).
struct Clips {
    clips: [Option<Animator>; 6],
    creature: bool,
    shown: Option<(usize, f32)>,
    fading: Option<(usize, f32, f64)>,
    acted: acting::Bound,
    shows: acting::Shows,
    flinch: acting::Flinch,
    attack: acting::Attack,
    throw: acting::Throw,
    strides: [f32; 2],
    cycles: [f32; 2],
    cycled_ms: Option<f64>,
    rest: Vec<(Option<usize>, Vec3, Quat)>,
    /// The nodes left as the last clip put them (`held_nodes`).
    held: Vec<bool>,
    lean: Option<Lean>,
}

const PAUSE: usize = 0;
const WALK: usize = 1;
const RUN: usize = 2;
const DIE: usize = 3;
const DEAD: usize = 4;
const GETUP: usize = 5;
/// Past the clips above, the ready stance of wield number `n` at `READY_AT`
/// + `n` (`acting::READY` + `n` among its scripted clips).
const READY_AT: usize = 6;
/// Past the ready stances, its walk with a rifle held up at `RIFLE_AT` and
/// its run at the next (`acting::RIFLE_GAIT` among its scripted clips).
const RIFLE_AT: usize = READY_AT + 100;

/// How a body is posed.
enum Look {
    /// JA's animations retargeted onto a KOTOR body; by weapon slot, the
    /// rig that grips the slot's blade while it is out.
    /// `acted`: its KOTOR clips for scripts' animations, and what it shows
    /// of them (`acting`) and of its grenade throws; a player's body its own
    /// clips out of a fight (`gait`).
    Retargeted {
        rig: Arc<BodyRig>,
        gripped: [Option<Arc<BodyRig>>; 2],
        acted: acting::Bound,
        shows: Box<acting::Shows>,
        throw: acting::Throw,
        draw: Box<acting::Once>,
        flinch: Box<acting::Flinch>,
        gait: Option<Box<crate::gait::Gait>>,
    },
    /// JA's own model (`--body ja`): its node `i + 1` is GLA bone `i`.
    Ja,
    /// A KOTOR creature's own clips.
    Clips(Box<Clips>),
}

impl Look {
    fn retargeted(rig: Arc<BodyRig>, acted: acting::Bound) -> Look {
        Look::Retargeted {
            rig,
            gripped: [None, None],
            acted,
            shows: Box::default(),
            throw: acting::Throw::default(),
            draw: Box::default(),
            flinch: Box::default(),
            gait: None,
        }
    }
}

/// A character's body in the scene: its model's root and node entities,
/// how it is posed, and the ragdoll it falls as (`ragdolls`).
struct Body {
    root: Entity,
    nodes: Vec<Entity>,
    look: Look,
    doll: Option<Arc<Doll>>,
    /// Its `camerahook` node, its model's else the head's hung on it:
    /// where the dialog camera finds its head (`play_view::head_point`);
    /// without one, how high its model stands (`model_height`).
    camera: Option<Entity>,
    /// Its own model's `rbullet` and `lbullet` nodes: where a creature with
    /// a built-in gun (a mining droid's lasers, T3-M4's) fires from with
    /// its right and left hand. Both games' executables fire a shot from
    /// the body's node when the model has one, before the held weapon's
    /// `bullethook`.
    bullets: [Option<Entity>; 2],
    height: Option<f32>,
    /// Its head hit location's node, and how far its model's top stands over
    /// it at rest (metres), where it has both.
    head_node: Option<Entity>,
    head_rise: Option<f32>,
    /// The node of its model each hit location's flinch turns (the first of
    /// the location's `Spot::nodes` it has), and its upper torso's.
    flinch_nodes: ([Option<usize>; LOCATIONS], Option<usize>),
    /// Its model's index among the scene's models.
    model: usize,
    /// The bone its head turns by in a conversation (`head_bone`).
    head: Option<HeadBone>,
    /// Its hung head's face, posed by its clips (`face`).
    face: Option<crate::face::Face>,
    /// For a body whose emitters or lights its clips key (`own_fx`), the
    /// KOTOR clips it was posed in last and how far into each (seconds): a
    /// droid's or beast's own and a script's over it, a humanoid's
    /// scripted and gait clips; empty when it was not posed.
    playing: Option<Vec<(Arc<AnimationIr>, f32)>>,
}

/// A character's body as drawn (`Cast::drawn_body`).
pub struct DrawnBody<'a> {
    pub root: Entity,
    pub yaw: Option<f32>,
    pub kotor: Option<(usize, &'a [Entity])>,
}

/// A drawn character: its body and its animation state (its weapons are
/// in `Holdings`); while it is drawn in a disguise's body, its own, hidden.
struct Member {
    body: Body,
    figure: Figure,
    own: Option<Body>,
    /// The events its KOTOR clip passed since they were taken (`heard`).
    heard: Vec<crate::clip_events::Heard>,
    /// As its snapshots last had it while dead: a body the sim has taken
    /// away goes on playing its death from there (`playing_death`).
    gone: Option<Blended>,
    /// The floor last found under it (`ground_lean::floor_under`), and from
    /// where (its middle and soles): a body standing still finds the same
    /// floor without tracing for it again.
    floor: Option<((Vec3, Vec3), Option<Floor>)>,
}

impl Member {
    /// Its body's scale against JA's, where JA's clips drive it.
    fn ja_scale(&self) -> Option<f32> {
        match &self.body.look {
            Look::Retargeted { rig, .. } => Some(rig.retargeter.scale()),
            _ => None,
        }
    }

    /// Where its body's nodes a grabber's hand may touch are drawn (world,
    /// metres), as they stand now, with its scale
    /// (`grapple_view::Held`); `None` without them all.
    fn grapple_nodes(
        &self,
        transforms: &Query<&mut Transform>,
    ) -> Option<crate::grapple_view::Held> {
        let Look::Retargeted { rig, .. } = &self.body.look else {
            return None;
        };
        let model = rig.model();
        let mut nodes = [Vec3::ZERO; 6];
        for (at, (_, name)) in nodes.iter_mut().zip(asset_ja::grapple::VICTIM_BONES) {
            let node = model.node_index(name)?;
            *at = drawn_at(
                transforms,
                self.body.root,
                (&self.body.nodes, &rig.rest),
                node,
            )?;
        }
        Some(crate::grapple_view::Held {
            nodes,
            scale: rig.retargeter.scale(),
        })
    }

    /// Whether its death still plays at `ms` (`DEATH_PLAYS_MS` since its
    /// snapshots first had it dead): it is posed whether or not the camera
    /// is near or the sim still has it, so it lies as its death leaves it,
    /// never held in the pose it had when it went.
    fn playing_death(&self, ms: f64) -> bool {
        self.figure
            .died()
            .is_some_and(|died| ms < died as f64 + DEATH_PLAYS_MS)
    }
}

/// How long a death plays out (ms): longer than any dying clip, JA's or
/// KOTOR's, and a ragdoll's fall.
const DEATH_PLAYS_MS: f64 = 10_000.0;

/// How many hit locations there are (`Location` as an index).
pub const LOCATIONS: usize = Location::ALL.len();

/// What the view makes of a hit location (`spot`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// The KOTOR body nodes that stand for it, best first.
    pub nodes: &'static [&'static str],
    /// The ragdoll part a hit there strikes.
    pub struck: &'static str,
    /// The humanoid part a killing cut there takes off, by a coin's two
    /// sides, as JA maps its hit locations to the parts it can cut (its
    /// right hand at the forearm, the ragdoll's hand being the forearm's; a
    /// hit to the chest or back taking the head or the body in half).
    pub cut: [&'static str; 2],
    /// Where on a body fitted without a plan a hit there lands: across its
    /// width from its left (0) to its right (1) and up its height.
    pub landing: [f32; 2],
}

/// Every hit location's `Spot`, in one table.
pub fn spot(location: Location) -> Spot {
    const TORSO: &[&str] = &["torsoupr_g", hooks::TORSO];
    let spot = |nodes, struck, cut: [&'static str; 2], landing| Spot {
        nodes,
        struck,
        cut,
        landing,
    };
    let middle = |struck| spot(TORSO, struck, ["head", "belly"], [0.5, 0.55]);
    let side = |left: bool| match left {
        true => spot(TORSO, "chest", ["upper arm left"; 2], [0.0, 0.75]),
        false => spot(TORSO, "chest", ["upper arm right"; 2], [1.0, 0.75]),
    };
    match location {
        Location::None | Location::Chest | Location::Back => middle("chest"),
        Location::ChestLeft | Location::BackLeft => side(true),
        Location::ChestRight | Location::BackRight => side(false),
        // A body whose head is a model of its own holds it at `headhook`.
        Location::Head => spot(
            &[hooks::HEAD, hooks::HEAD_HOOK],
            "head",
            ["head"; 2],
            [0.5, 1.0],
        ),
        Location::Waist => spot(
            &[hooks::TORSO, hooks::PELVIS],
            "belly",
            ["belly"; 2],
            [0.5, 0.55],
        ),
        Location::ArmLeft => spot(
            &["lforearm_g", "lbicep_g"],
            "upper arm left",
            ["upper arm left"; 2],
            [0.0, 0.75],
        ),
        Location::ArmRight => spot(
            &["rforearm_g", "rbicep_g"],
            "upper arm right",
            ["upper arm right"; 2],
            [1.0, 0.75],
        ),
        Location::HandLeft => spot(
            &["lhand_g", hooks::LEFT_HAND],
            "forearm left",
            ["upper arm left"; 2],
            [0.0, 0.5],
        ),
        Location::HandRight => spot(
            &["rhand_g", hooks::RIGHT_HAND],
            "forearm right",
            ["forearm right"; 2],
            [1.0, 0.5],
        ),
        Location::LegLeft => spot(
            &["lshin_g", "lthigh_g"],
            "thigh left",
            ["thigh left"; 2],
            [0.25, 0.2],
        ),
        Location::LegRight => spot(
            &["rshin_g", "rthigh_g"],
            "thigh right",
            ["thigh right"; 2],
            [0.75, 0.2],
        ),
        Location::FootLeft => spot(&["lfoot_g"], "shin left", ["thigh left"; 2], [0.25, 0.2]),
        Location::FootRight => spot(&["rfoot_g"], "shin right", ["thigh right"; 2], [0.75, 0.2]),
    }
}

/// Where each character's hit locations are on its body: the node standing
/// for each location, or `None` where the body has none (a droid's), by
/// character. The combat effects put a hit there. The second field is the
/// characters whose heads are armour (a droid's, a Sith trooper's helmet),
/// which bolts may burn like the rest of the body.
#[derive(Resource, Default)]
pub struct Hitboxes(
    pub BTreeMap<SimId, [Option<Entity>; LOCATIONS]>,
    pub std::collections::BTreeSet<SimId>,
);

/// Where each character's shots leave its guns as drawn (`Wielded::guns`):
/// each gun model's root and the muzzle in its space (metres), by
/// character and the hand a shot names (a pair's right and left; else the
/// gun out first). The combat effects start its flashes and bolts there.
#[derive(Resource, Default)]
pub struct Guns(pub BTreeMap<SimId, [Option<(Entity, Vec3)>; 2]>);

/// The body model's nodes a shot from the right and the left hand leaves
/// from, when the model has them (`Body::bullets`).
const BULLET_NODES: [&str; 2] = ["rbullet", "lbullet"];

/// Where each hand's shots leave: the body's own node (`own`) where it has
/// one, as both games' executables look there first, else the gun `held`
/// in that hand.
fn muzzles(
    own: [Option<Entity>; 2],
    held: [Option<(Entity, Vec3)>; 2],
) -> [Option<(Entity, Vec3)>; 2] {
    std::array::from_fn(|hand| own[hand].map(|node| (node, Vec3::ZERO)).or(held[hand]))
}

/// The probes of where shots leave a body (`kjk sim probes`, group
/// "muzzles").
#[cfg(feature = "probes")]
pub fn muzzle_probes() -> sim::probe::Checks {
    let mut world = World::new();
    let [body, left, gun] = [(); 3].map(|()| world.spawn_empty().id());
    let out = Vec3::new(0.0, 0.3, 0.0);
    let droid = muzzles([Some(body), Some(left)], [None; 2]);
    let armed = muzzles([None; 2], [Some((gun, out)), None]);
    let both = muzzles([Some(body), None], [Some((gun, out)), None]);
    vec![
        (
            "a droid fires from its rbullet and lbullet nodes",
            sim::probe::check(
                droid == [Some((body, Vec3::ZERO)), Some((left, Vec3::ZERO))],
                format!("{droid:?}"),
            ),
        ),
        (
            "a body with no such node fires from its gun",
            sim::probe::check(armed == [Some((gun, out)), None], format!("{armed:?}")),
        ),
        (
            "the body's node comes before a held gun",
            sim::probe::check(both[0] == Some((body, Vec3::ZERO)), format!("{both:?}")),
        ),
    ]
}

/// Each drawn character's body model's root, by character: the script
/// effects' glows cover its meshes (`script_fx`), and so does the look of
/// a character in stealth (`stealth_view`).
#[derive(Resource, Default)]
pub struct BodyRoots(pub BTreeMap<SimId, Entity>);

/// Each drawn character's head hit location's node, by character: the
/// drawn head as posed, which the overhead bars ride on, and how far its
/// model's top stands over it at rest (metres). A body without a head node
/// (a droid's) is not in it.
#[derive(Resource, Default)]
pub struct BodyHeads(pub BTreeMap<SimId, (Entity, f32)>);

/// How high each drawn character's model stands over its soles, at rest
/// (metres, `Cast::model_height`), by character. A character whose model has
/// no meshes is not in it.
#[derive(Resource, Default)]
pub struct BodyHeights(pub BTreeMap<SimId, f32>);

/// Where the players' bodies are among the scene's models: each seat's as
/// the scene was composed (`play::compose`), for `seats` seats.
#[derive(Clone)]
pub struct Bodies {
    pub at: Vec<usize>,
    pub seats: u8,
    /// Seats dressed again in this scene (`wardrobe`): their bodies now.
    pub dressed: BTreeMap<Seat, usize>,
}

impl Bodies {
    fn of(&self, seat: Seat) -> Option<usize> {
        if seat.0 >= self.seats {
            return None;
        }
        self.dressed
            .get(&seat)
            .or_else(|| self.at.get(usize::from(seat.0)))
            .copied()
    }
}

/// The sim's blades in hand of the characters posed, each with its hand:
/// what their gripping hands reach for (`Cast::draw`).
pub type Reaching = BTreeMap<SimId, Vec<(Blade, Hand)>>;

/// Every character's body and figure.
pub struct Cast {
    pub rig: Rig,
    /// The camera's player's seat.
    pub seat: Seat,
    /// The players' bodies and each seat's rig in turn (none for JA's own
    /// model).
    bodies: Bodies,
    player_rigs: Vec<Arc<BodyRig>>,
    /// By character; `None` for one with nothing to draw it.
    members: BTreeMap<SimId, Option<Member>>,
    /// The doors' states the members' floors were found with: another
    /// state, and they are found again.
    floor_states: Option<Vec<u8>>,
    /// Who holds which weapon.
    holdings: Holdings,
    /// Each body model lined up once, by model name (and grip).
    rigs: BTreeMap<String, Arc<BodyRig>>,
    /// The characters posed this frame, how long placing and posing
    /// everyone took, and the part of it their weapons took.
    pub posed: usize,
    pub pose_ms: f64,
    pub arms_ms: f64,
    pub totals: PoseTotals,
    /// Frames posed so far: whose turn it is beyond the nearest.
    round: usize,
    small_posed: BTreeMap<SimId, std::time::Instant>,
    /// The dead, falling and fallen.
    pub ragdolls: Ragdolls,
    /// Limbs cut off this frame, for the scene to part their bodies'
    /// meshes (`severing`): each body's nodes, its doll, the part.
    pub cuts: Vec<crate::severing::Owed>,
    /// The camera's player's eyes (game space, metres from its eye height
    /// over the origin) while it looks through them (`first_person`): its
    /// gun is held up before them.
    pub eye: Option<Vec3>,
    /// The bolt hits of the last moments (`bolt_hits::BoltHits`): each body
    /// struck flinches from them.
    pub bolt_hits: Vec<crate::bolt_hits::BoltHit>,
    /// How the player's own gun, seen from behind, lines up with the point
    /// under the crosshair as last drawn (`aim_pose::Sight`), for the dumps.
    pub sight: Option<crate::aim_pose::Sight>,
    /// The game's base items' wields: the stance each weapon breathes in.
    wields: stance::Wields,
    /// The bodies the conversations under way hide items of (S8).
    unequipped: Vec<Unequipped>,
    /// Each conversation's speaker and since when (sim ms) it has spoken
    /// its lines running (`note_talking`).
    talking: Vec<Talking>,
    /// The KOTOR clips each body posed plays this frame and how far into
    /// each (seconds), by its model: what keys its emitters and lights
    /// (`own_fx::BodyClips`).
    pub body_clips: BTreeMap<usize, Vec<(Arc<AnimationIr>, f32)>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PoseTotals {
    pub posed: u64,
    pub ms: f64,
    pub frames: u64,
}

impl PoseTotals {
    fn add(&mut self, posed: usize, ms: f64) {
        self.posed += posed as u64;
        self.ms += ms;
        self.frames += 1;
    }
}

/// A body some line of a conversation (its `owner`'s, of `dialog`) has had
/// as speaker or listener, whose DLG hides its item models until the
/// conversation ends.
struct Unequipped {
    owner: ObjectId,
    dialog: String,
    who: SimId,
    hide: Hide,
}

/// The speaker of a conversation's lines (`Conversation::speaker`, `id`'s)
/// and since when (sim ms) it has said them one after another.
struct Talking {
    id: u32,
    who: SimId,
    since: u64,
}

/// The camera's player: the body `seat` plays (`Snapshot::body`: its own
/// character, or the party member it drives), none while it watches.
pub fn me(snapshot: &Snapshot, seat: Seat) -> Option<SimId> {
    snapshot.body(seat).map(|entity| entity.id)
}

impl Cast {
    pub fn new(
        rig: Rig,
        bodies: Bodies,
        player_rigs: Vec<Arc<BodyRig>>,
        wields: stance::Wields,
    ) -> Cast {
        Cast {
            rig,
            seat: Seat(0),
            bodies,
            player_rigs,
            members: BTreeMap::new(),
            floor_states: None,
            holdings: Holdings::default(),
            rigs: BTreeMap::new(),
            posed: 0,
            pose_ms: 0.0,
            arms_ms: 0.0,
            totals: PoseTotals::default(),
            round: 0,
            small_posed: BTreeMap::new(),
            ragdolls: Ragdolls::default(),
            cuts: Vec::new(),
            eye: None,
            bolt_hits: Vec::new(),
            sight: None,
            wields,
            unequipped: Vec::new(),
            talking: Vec::new(),
            body_clips: BTreeMap::new(),
        }
    }

    /// A new world: every character starts over, the players on their
    /// bodies there (`bodies`, each seat's). In the same scene (`kept`) each
    /// takes the weapons it held when it is drawn again
    /// (`Holdings::reset`). Blades' indices returned to retire.
    /// Seat `seat`'s body model, in the scene's models.
    pub fn body_of(&self, seat: Seat) -> Option<usize> {
        self.bodies.of(seat)
    }

    pub fn reset(
        &mut self,
        bodies: Option<&[usize]>,
        kept: bool,
        commands: &mut Commands,
    ) -> Vec<usize> {
        if let Some(bodies) = bodies {
            self.bodies.at = bodies.to_vec();
            self.bodies.dressed.clear();
        }
        // Another world's floors are found anew.
        for member in self.members.values_mut().flatten() {
            member.floor = None;
        }
        // The disguised go back to their own bodies (in the same scene,
        // the disguises' wait hidden for them to be cast again).
        for member in self.members.values().flatten() {
            if let Some(own) = &member.own {
                commands.entity(own.root).try_insert(Visibility::Inherited);
                commands
                    .entity(member.body.root)
                    .try_insert(Visibility::Hidden);
            }
        }
        self.members.clear();
        self.ragdolls = Ragdolls::default();
        self.cuts.clear();
        self.small_posed.clear();
        self.holdings.reset(kept, commands)
    }

    /// The scene's composed weapons, once per scene (`Holdings::seed`):
    /// before anything is armed or drawn in it.
    pub fn seed(&mut self, scene: &LoadedScene, models: &[Option<(Entity, Vec<Entity>)>]) {
        self.holdings.seed(scene.arms.iter().map(|(&owner, arms)| {
            let armed = arms
                .each_ref()
                .map(|held| held.as_ref().and_then(|held| held.armed(models)));
            (owner, armed)
        }));
    }

    /// Every drawn character's guns (`Guns`): for each hand its body's own
    /// muzzle node (`Body::bullets`), else the gun it holds there.
    pub fn guns(&self) -> BTreeMap<SimId, [Option<(Entity, Vec3)>; 2]> {
        self.members
            .iter()
            .filter_map(|(&id, member)| {
                let held = (self.holdings.wielded(id)).map_or([None; 2], |wielded| wielded.guns());
                let own = member
                    .as_ref()
                    .map_or([None; 2], |member| member.body.bullets);
                let guns = muzzles(own, held);
                guns.iter().any(Option::is_some).then_some((id, guns))
            })
            .collect()
    }

    /// Whether `id` has a gun readied: drawn for a fight, not only carried
    /// or holstered.
    pub fn gun_readied(&self, id: SimId) -> bool {
        self.figure(id).is_some_and(|figure| figure.readied)
            && self
                .holdings
                .wielded(id)
                .is_some_and(|wielded| wielded.guns().iter().any(Option::is_some))
    }

    /// Every drawn character's body root (`BodyRoots`).
    pub fn roots(&self) -> BTreeMap<SimId, Entity> {
        self.members
            .iter()
            .filter_map(|(&id, member)| Some((id, member.as_ref()?.body.root)))
            .collect()
    }

    /// Every drawn character's head node and its rise (`BodyHeads`).
    pub fn heads(&self) -> BTreeMap<SimId, (Entity, f32)> {
        self.members
            .iter()
            .filter_map(|(&id, member)| {
                let body = &member.as_ref()?.body;
                Some((id, (body.head_node?, body.head_rise?)))
            })
            .collect()
    }

    /// Every drawn character's model height (`BodyHeights`).
    pub fn heights(&self) -> BTreeMap<SimId, f32> {
        self.members
            .keys()
            .filter_map(|&id| Some((id, self.model_height(id)?)))
            .collect()
    }

    pub fn figure(&self, id: SimId) -> Option<&Figure> {
        Some(&self.members.get(&id)?.as_ref()?.figure)
    }

    /// Every character's body that may be drawn, for its shadow: the root,
    /// its model in the scene's models, the node entities, the ragdoll it
    /// is fitted with, and the ragdoll's parts that a cut took off
    /// (`Ragdolls::severed`). A disguised character gives both its bodies,
    /// and only the one shown casts. Only the body that fell can be cut.
    pub fn shadow_bodies(
        &self,
    ) -> impl Iterator<Item = (Entity, usize, &[Entity], Option<&Doll>, u64)> {
        self.members
            .iter()
            .filter_map(|(&id, member)| Some((id, member.as_ref()?)))
            .flat_map(|(id, member)| {
                let severed = self.ragdolls.severed(id);
                std::iter::once((&member.body, severed))
                    .chain(member.own.iter().map(|own| (own, 0)))
            })
            .map(|(body, severed)| {
                (
                    body.root,
                    body.model,
                    body.nodes.as_slice(),
                    body.doll.as_deref(),
                    severed,
                )
            })
    }

    /// How many characters have a body to draw them.
    pub fn drawn(&self) -> usize {
        self.members.values().flatten().count()
    }

    /// The first seat's body rig (the trace follows it).
    pub fn player_rig(&self) -> Option<&BodyRig> {
        self.player_rigs.first().map(Arc::as_ref)
    }

    /// Finds the bodies of the characters in `snapshot` not seen yet (the
    /// scene's models by index: root and nodes), their hit locations'
    /// nodes, and gives each drawn one a figure.
    pub fn cast(
        &mut self,
        snapshot: &Snapshot,
        level: &sim::Level,
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        hitboxes: &mut Hitboxes,
        commands: &mut Commands,
    ) {
        let objects = Instances::in_snapshot(level, snapshot);
        for entity in snapshot.all_drawn() {
            let Some(player) = entity.player else {
                continue;
            };
            if self.members.contains_key(&entity.id) {
                continue;
            }
            let droid = droid(entity, &objects);
            let Some(mut body) = self.body((entity, droid), scene, models, commands) else {
                self.members.insert(entity.id, None);
                continue;
            };
            let carries = matches!(entity.kind, Kind::Avatar(_));
            let hands = self.fit(
                (entity.id, carries, droid),
                &mut body,
                scene,
                models,
                hitboxes,
            );
            let owner = match entity.kind {
                Kind::Avatar(seat) => Some(Owner::Seat(seat.0)),
                Kind::Object(_, index) => Some(Owner::Creature(index)),
                _ => None,
            };
            self.holdings.wield(
                (entity.id, owner),
                &sheet(entity, &objects),
                hands,
                commands,
            );
            self.grip(&mut body, entity.id);
            let ready = ready_anim(weapon(entity, &objects));
            let member = Member {
                body,
                figure: Figure::new(&self.rig, player.view[1], ready),
                own: None,
                heard: Vec::new(),
                gone: None,
                floor: None,
            };
            self.members.insert(entity.id, Some(member));
        }
    }

    /// Each seat's body (the scene composes one per seat, standing at the
    /// stage's spawn, its head and weapons hung under it) shown only while
    /// `snapshot` has that seat's own character, undisguised: a seat that
    /// plays a companion, watches or has left draws no figure of its own.
    /// Seats shown this frame.
    pub fn show_seats(
        &self,
        snapshot: &Snapshot,
        models: &[Option<(Entity, Vec<Entity>)>],
        visibility: &mut Query<&mut Visibility>,
    ) -> u8 {
        let mut shown = 0;
        for seat in (0..self.bodies.seats).map(Seat) {
            let Some((root, _)) = self.bodies.of(seat).and_then(|at| models.get(at)?.as_ref())
            else {
                continue;
            };
            let has = snapshot.own(seat).filter(|entity| !snapshot.away(entity));
            // A disguised seat is drawn in the disguise's body.
            let own = has.is_some_and(|entity| !self.disguised(entity.id));
            shown += u8::from(own);
            show(visibility, *root, own);
        }
        shown
    }

    /// Whether character `id` is drawn in a disguise's body.
    fn disguised(&self, id: SimId) -> bool {
        matches!(self.members.get(&id), Some(Some(member)) if member.own.is_some())
    }

    /// Each living character drawn in the body it wears: a disguise's while
    /// `worn` gives one (`made::Made::disguise`, once the scene has it),
    /// else its own. Its figure carries on in the new body, which its
    /// weapons now hang from; the body it leaves is hidden (its own kept for
    /// when the disguise comes off). The dead keep the body they fell in.
    pub fn disguise<'v>(
        &mut self,
        (snapshot, level): (&Snapshot, &sim::Level),
        worn: impl Fn(SimId) -> Option<&'v CreatureView>,
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        (hitboxes, visibility): (&mut Hitboxes, &mut Query<&mut Visibility>),
        commands: &mut Commands,
    ) {
        let objects = Instances::in_snapshot(level, snapshot);
        for entity in &snapshot.entities {
            let id = entity.id;
            let Some(Some(member)) = self.members.get(&id) else {
                continue;
            };
            let wanted = worn(id).filter(|view| models.get(view.body).is_some_and(Option::is_some));
            let wears = member.own.as_ref().map(|_| member.body.model);
            if wears == wanted.map(|view| view.body) || entity.fighter.is_some_and(|f| f.dead()) {
                continue;
            }
            let Some(Some(mut member)) = self.members.remove(&id) else {
                continue;
            };
            let sheet = sheet(entity, &objects);
            let droid = droid(entity, &objects);
            let body = match wanted {
                Some(view) => self.body_in(view, droid, scene, models, commands),
                None => member.own.take(),
            };
            if let Some(mut body) = body {
                let carries = matches!(entity.kind, Kind::Avatar(_));
                let hands = self.fit((id, carries, droid), &mut body, scene, models, hitboxes);
                self.holdings.rehang(id, &sheet, hands, commands);
                self.grip(&mut body, id);
                show(visibility, member.body.root, false);
                show(visibility, body.root, true);
                let left = std::mem::replace(&mut member.body, body);
                if wanted.is_some() && member.own.is_none() {
                    member.own = Some(left);
                }
            }
            self.members.insert(id, Some(member));
        }
    }

    /// `body`'s rig gripping the blades among character `id`'s weapons.
    fn grip(&mut self, body: &mut Body, id: SimId) {
        let (Look::Retargeted { rig, gripped, .. }, Some(wielded)) =
            (&mut body.look, self.holdings.wielded(id))
        else {
            return;
        };
        let dual = wielded.dual();
        let doubles = [0, 1].map(|slot| wielded.blade(slot).map(|blade| blade.double));
        let rig = rig.clone();
        for (grip, double) in gripped.iter_mut().zip(doubles) {
            *grip = double.and_then(|double| self.gripped(&rig, double, dual));
        }
    }

    /// Arms `owner`'s character (`id`) again with `sheet`'s weapons
    /// (`arms`, spawned by the armory, hidden): what it had is retired,
    /// their blades' indices returned. One not drawn yet, or with no body
    /// to draw it so far (a creature made in play whose body is still
    /// loading, `made`), takes them when it is (`Holdings::rearm`).
    pub fn rearm(
        &mut self,
        (owner, id): (Owner, Option<SimId>),
        sheet: &Sheet,
        arms: [Option<Armed>; 2],
        commands: &mut Commands,
    ) -> Vec<usize> {
        match id.and_then(|id| Some((id, self.members.get_mut(&id)?.take()?))) {
            Some((id, mut member)) => {
                let retired = self
                    .holdings
                    .rearm((owner, Some(id)), sheet, arms, commands);
                self.grip(&mut member.body, id);
                self.members.insert(id, Some(member));
                retired
            }
            None => self.holdings.rearm((owner, id), sheet, arms, commands),
        }
    }

    /// Character `id` of `owner`, a creature made in play, has its body
    /// now (`made`), with its template's weapons (`arms`): it is cast in
    /// the next `cast`, taking them, or the armory's when those came first
    /// (`Holdings::arrive`). Blades' indices returned to retire.
    pub fn arrive(
        &mut self,
        (id, owner): (SimId, Owner),
        arms: [Option<Armed>; 2],
        commands: &mut Commands,
    ) -> Vec<usize> {
        self.recast(id);
        self.holdings.arrive(owner, arms, commands)
    }

    /// Character `id`'s body plays its wield's draw from now
    /// (`acting::Once`): its weapons changed in its hands.
    pub fn draws(&mut self, id: SimId) {
        if let Some(Some(Member {
            body:
                Body {
                    look: Look::Retargeted { draw, .. },
                    ..
                },
            ..
        })) = self.members.get_mut(&id)
        {
            draw.begin();
        }
    }

    /// Seat `seat` dressed again (`wardrobe`): from now on its body is the
    /// scene's model `body`, lined up as `rig`; its character `id` (when it
    /// is drawn) lets go of its weapons and is cast again on it, taking
    /// them in its new hands. Blades' indices returned to retire.
    pub fn redress(
        &mut self,
        (seat, id): (Seat, Option<SimId>),
        body: usize,
        rig: Arc<BodyRig>,
        commands: &mut Commands,
    ) -> Vec<usize> {
        self.bodies.dressed.insert(seat, body);
        let at = usize::from(seat.0);
        if self.player_rigs.len() <= at {
            let first = self.player_rigs.first().cloned().unwrap_or(rig.clone());
            self.player_rigs.resize(at + 1, first);
        }
        self.player_rigs[at] = rig;
        let Some(id) = id else {
            return Vec::new();
        };
        self.members.remove(&id);
        self.holdings.release(id, commands)
    }

    /// `id`'s body as drawn: its root entity, the yaw it faces (game
    /// degrees, while its figure has one), its model's index among the
    /// scene's models and its node entities; none for JA's own model.
    pub fn drawn_body(&self, id: SimId) -> Option<DrawnBody<'_>> {
        let member = self.members.get(&id)?.as_ref()?;
        let body = &member.body;
        Some(DrawnBody {
            root: body.root,
            yaw: member.figure.yaw(),
            kotor: (!matches!(body.look, Look::Ja)).then_some((body.model, body.nodes.as_slice())),
        })
    }

    /// `id`'s body's `camerahook` node, if it is drawn and has one.
    pub fn camera_node(&self, id: SimId) -> Option<Entity> {
        self.members.get(&id)?.as_ref()?.body.camera
    }

    /// How high `id`'s body stands over its soles (metres), if it is drawn
    /// with meshes (`model_height`).
    pub fn model_height(&self, id: SimId) -> Option<f32> {
        self.members.get(&id)?.as_ref()?.body.height
    }

    /// Character `id` cast again in the next `cast` when nothing drew it
    /// so far: a creature made in play whose body came after it did
    /// (`made` loads it off the frame).
    pub fn recast(&mut self, id: SimId) {
        if matches!(self.members.get(&id), Some(None)) {
            self.members.remove(&id);
        }
    }

    /// Character `id` of `owner` (a creature made in play, `made`) is gone
    /// with its body: its figure and weapons go too, their blades' indices
    /// returned to retire. Its ragdoll leaves the ragdolls' world. Kept, it
    /// would lie there unseen until the scene changed, in the way of the
    /// next bodies that fall.
    pub fn forget(&mut self, (id, owner): (SimId, Owner), commands: &mut Commands) -> Vec<usize> {
        self.members.remove(&id);
        self.ragdolls.rise(id);
        self.holdings.forget((id, owner), commands)
    }

    /// The character drawn in the scene's model `body`, if it died.
    pub fn fallen_in(&self, body: usize) -> Option<SimId> {
        self.members.iter().find_map(|(&id, member)| {
            let member = member.as_ref()?;
            (member.body.model == body && member.figure.died().is_some()).then_some(id)
        })
    }

    /// The character drawn in the scene's model `body` (creature `index`,
    /// placed by the module) is gone: forgotten as `forget` does, its id
    /// returned with the blades' indices to retire.
    pub fn forget_body(
        &mut self,
        (body, index): (usize, u32),
        commands: &mut Commands,
    ) -> Option<(SimId, Vec<usize>)> {
        let id = self.members.iter().find_map(|(&id, member)| {
            member
                .as_ref()
                .is_some_and(|member| member.body.model == body)
                .then_some(id)
        })?;
        Some((id, self.forget((id, Owner::Creature(index)), commands)))
    }

    /// `rig` holding blades (`BodyRig::gripped`): lined up once per body
    /// model and grip.
    fn gripped(&mut self, rig: &BodyRig, double: bool, dual: bool) -> Option<Arc<BodyRig>> {
        let key = format!(
            "{} gripping {double} {dual}",
            rig.ir.name.to_ascii_lowercase()
        );
        if !self.rigs.contains_key(&key) {
            let blade = self.rig.sabers.of(double).dir();
            let gripped = rig.gripped(&self.rig, blade, (double, dual))?;
            self.rigs.insert(key.clone(), Arc::new(gripped));
        }
        self.rigs.get(&key).cloned()
    }

    /// A character's body: its seat's play body for a player, a creature's
    /// own (`body_in`).
    fn body(
        &mut self,
        (entity, droid): (&EntityState, bool),
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        commands: &mut Commands,
    ) -> Option<Body> {
        match entity.kind {
            Kind::Avatar(seat) => {
                let model = self.bodies.of(seat)?;
                let rig = self
                    .player_rigs
                    .get(usize::from(seat.0) % self.player_rigs.len().max(1))
                    .cloned();
                let gait = rig.as_ref().map(|rig| rig.gait.clone());
                let look = rig.map_or(Look::Ja, |rig| {
                    let acted = match (scene.models.get(model), models.get(model)) {
                        (Some(placed), Some(Some((_, nodes)))) => {
                            acting::bind(placed, &rig.acted, nodes)
                        }
                        _ => Vec::new(),
                    };
                    Look::retargeted(rig, acted)
                });
                self.embodied((model, look, gait, droid), scene, models, commands)
            }
            Kind::Object(ObjectKind::Creature, index) => {
                let view = scene.creature_views.iter().find(|v| v.object == index)?;
                self.body_in(view, droid, scene, models, commands)
            }
            _ => None,
        }
    }

    /// The body `view` draws (a creature's own, or a disguise's): a
    /// humanoid's retargeted (each body model lined up once), any other's
    /// posed by its own clips.
    fn body_in(
        &mut self,
        view: &CreatureView,
        droid: bool,
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        commands: &mut Commands,
    ) -> Option<Body> {
        let placed = scene.models.get(view.body)?;
        let nodes = &models.get(view.body)?.as_ref()?.1;
        let look = if view.humanoid {
            let rig = self
                .rigs
                .entry(placed.ir.name.to_ascii_lowercase())
                .or_insert_with(|| {
                    let hold = placed.clip.as_deref().map_or_else(Vec::new, finger_hold);
                    let rig = BodyRig::new(&placed.ir, &self.rig, hold);
                    Arc::new(rig.with_acted(view.clips.acted.clone()))
                });
            Look::retargeted(rig.clone(), acting::bind(placed, &view.clips.acted, nodes))
        } else {
            let bind = |clip: &Option<Arc<AnimationIr>>| {
                clip.as_ref()
                    .and_then(|clip| crate::scene::animator(placed, clip, nodes))
            };
            let CreatureClips {
                pause,
                walk,
                run,
                die,
                dead,
                getup,
                acted,
                strides,
                creature,
            } = &view.clips;
            Look::Clips(Box::new(Clips {
                clips: [pause, walk, run, die, dead, getup].map(bind),
                creature: *creature,
                shown: None,
                fading: None,
                acted: acting::bind(placed, acted, nodes),
                shows: acting::Shows::default(),
                flinch: acting::Flinch::default(),
                attack: acting::Attack::default(),
                throw: acting::Throw::default(),
                strides: *strides,
                cycles: [0.0; 2],
                cycled_ms: None,
                rest: rest_of(&placed.ir),
                held: held_nodes(&placed.ir, pause.as_deref()),
                lean: Lean::new(view.ground_tilt),
            }))
        };
        let gait = crate::gait::GaitClips {
            clips: [&view.clips.pause, &view.clips.walk, &view.clips.run].map(Clone::clone),
            strides: view.clips.strides,
        };
        self.embodied(
            (view.body, look, Some(gait), droid),
            scene,
            models,
            commands,
        )
    }

    /// Model `model` as a body posed by `look`, with the ragdoll it falls
    /// as; one placed playing its idle clip (a creature's, a disguise's)
    /// stops playing it.
    fn embodied(
        &mut self,
        (model, look, gait, droid): (usize, Look, Option<crate::gait::GaitClips>, bool),
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        commands: &mut Commands,
    ) -> Option<Body> {
        let (root, nodes) = models.get(model)?.clone()?;
        let placed = scene.models.get(model)?;
        // Its own clips out of a fight (`gait`): a player's from its body
        // rig, a humanoid creature's (or disguise's) from its view.
        let mut look = look;
        let face = Self::face(
            (model, root),
            (&look, gait.as_ref()),
            scene,
            models,
            commands,
        );
        let hung = crate::face::hung_head(&scene.models, model).and_then(|head| {
            let (head_root, head_nodes) = models.get(head)?.as_ref()?;
            let placed = scene.models.get(head)?;
            let hung_at = placed.hook_local.as_ref().map_or(Quat::IDENTITY, |local| {
                Transform::from_matrix(crate::scene::to_mat4(local)).rotation
            });
            Some(Hung {
                ir: &placed.ir,
                nodes: &head_nodes[..],
                hook: placed.hook?.1,
                root: (*head_root, hung_at),
            })
        });
        let head = head_bone((&placed.ir, &nodes), hung);
        if let Look::Retargeted { gait: bound, .. } = &mut look {
            *bound = gait.map(|clips| crate::gait::Gait::bound(placed, &clips, &nodes));
        }
        if placed.clip.is_some() {
            commands.entity(root).remove::<Animator>();
            // What the idle clip left on the nodes the retarget does not pose
            // (the weapon nodes among them) goes back to rest.
            if let Look::Retargeted { rig, .. } = &look {
                for (&node, &(_, translation, rotation)) in nodes.iter().zip(&rig.rest) {
                    commands
                        .entity(node)
                        .insert(Transform::from_translation(translation).with_rotation(rotation));
                }
            }
        }
        let doll = match &look {
            Look::Retargeted { rig, .. } => self.ragdolls.humanoid(rig),
            Look::Clips(_) => {
                let material = if droid {
                    ragdoll::Material::Metal
                } else {
                    ragdoll::Material::Flesh
                };
                self.ragdolls.creature(&placed.ir, material)
            }
            Look::Ja => None,
        };
        Some(Body {
            root,
            nodes,
            look,
            doll,
            camera: None,
            bullets: [None; 2],
            height: None,
            head_node: None,
            head_rise: None,
            flinch_nodes: ([None; LOCATIONS], None),
            model,
            head,
            face,
            playing: (scene.own_fx.iter())
                .any(|own| own.model == model && matches!(own.plays, crate::viewer::Plays::Body))
                .then(Vec::new),
        })
    }

    /// The face of the head hung on body model `model` (`face::Face`),
    /// bound to every KOTOR clip `look` and its `gait` play, the gait's
    /// standing clip where none plays and its dead one once dead; the
    /// head's own idle loop stops, as the body's does.
    fn face(
        (model, root): (usize, Entity),
        (look, gait): (&Look, Option<&crate::gait::GaitClips>),
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        commands: &mut Commands,
    ) -> Option<crate::face::Face> {
        let head = crate::face::hung_head(&scene.models, model)?;
        let (head_root, nodes) = models.get(head)?.as_ref()?;
        let placed = scene.models.get(head)?;
        if placed.clip.is_some() {
            commands.entity(*head_root).remove::<Animator>();
        }
        let gaits = gait
            .into_iter()
            .flat_map(|gait| gait.clips.iter().flatten());
        let (acted, own): (&acting::Bound, Vec<&Animator>) = match look {
            Look::Retargeted { acted, .. } => (acted, Vec::new()),
            Look::Clips(clips) => (&clips.acted, clips.clips.iter().flatten().collect()),
            Look::Ja => return None,
        };
        let clips = (acted.iter().map(|(_, animator)| animator))
            .chain(own)
            .map(Animator::clip)
            .chain(gaits);
        let standing = gait.and_then(|gait| gait.clips[0].as_ref());
        let dead = (scene.creature_views.iter())
            .find(|view| view.body == model)
            .and_then(|view| view.clips.dead.as_ref());
        crate::face::Face::new(
            (placed, nodes),
            clips,
            [standing, dead],
            crate::stance::phase(root),
        )
    }

    /// `body`'s nodes the views find its character `id` by: its hit
    /// locations' (into `hitboxes`) and its `camerahook`, the body model's
    /// else the one of the head hung on it (`models` the scene's, by
    /// index); and the hands its weapons hang from, and where it carries one
    /// not in hand if it `carries` (the players' characters; everyone
    /// else's stay in hand, `Hands::carries`). A `droid`'s head, or one its
    /// models name as a helmet (`bolt_hits::armoured_head`), is armour, and
    /// bolts may burn it (`Hitboxes::1`).
    fn fit(
        &self,
        (id, carries, droid): (SimId, bool, bool),
        body: &mut Body,
        scene: &LoadedScene,
        models: &[Option<(Entity, Vec<Entity>)>],
        hitboxes: &mut Hitboxes,
    ) -> Hands {
        let ir = &scene.models[body.model].ir;
        let node = |name: &str| {
            let node = ir.node_index(name)?;
            body.nodes.get(node).copied()
        };
        let spots =
            Location::ALL.map(|location| spot(location).nodes.iter().find_map(|name| node(name)));
        hitboxes.0.insert(id, spots);
        let head_hook = ir.node_index(hooks::HEAD_HOOK);
        let mut names = std::iter::once(ir.name.as_str()).chain(
            (scene.models.iter())
                .filter(|placed| {
                    (placed.hook).is_some_and(|(on, at)| on == body.model && Some(at) == head_hook)
                })
                .map(|placed| placed.ir.name.as_str()),
        );
        if droid || names.any(crate::bolt_hits::armoured_head) {
            hitboxes.1.insert(id);
        } else {
            hitboxes.1.remove(&id);
        }
        let index = |name: &str| ir.node_index(name).filter(|&at| at < body.nodes.len());
        body.flinch_nodes = (
            Location::ALL.map(|location| spot(location).nodes.iter().find_map(|name| index(name))),
            index(UPPER_TORSO),
        );
        body.camera = node(hooks::CAMERA_HOOK).or_else(|| hung_camera(body.model, scene, models));
        body.bullets = BULLET_NODES.map(node);
        body.height = model_height(body.model, scene);
        let head = spot(Location::Head)
            .nodes
            .iter()
            .find_map(|name| ir.node_index(name));
        body.head_node = head.and_then(|at| body.nodes.get(at).copied());
        body.head_rise = body.height.zip(head).map(|(top, at)| {
            top - crate::scene::to_mat4(&ir.nodes[at].world)
                .transform_point3(Vec3::ZERO)
                .z
        });
        let ja = matches!(body.look, Look::Ja);
        let ja_hands = ja.then_some(&self.rig.hands);
        let stowed = |stow| {
            weapons::stow_nodes(ir, ja, stow).map(|at| {
                at.filter(|_| carries)
                    .and_then(|(node, place)| Some((*body.nodes.get(node)?, place)))
            })
        };
        Hands {
            carries,
            held: weapons::hand_nodes(ir, ja_hands).map(|hand| {
                let (node, local) = hand?;
                Some((
                    *body.nodes.get(node)?,
                    Transform::from_matrix(crate::scene::to_mat4(&local)),
                ))
            }),
            hips: stowed(Stow::Hip),
            thighs: stowed(Stow::Thigh),
            holsters: weapons::stow_nodes(ir, ja, Stow::Thigh).map(|at| at.filter(|_| carries)),
            backs: stowed(Stow::Back),
            staffs: stowed(Stow::Staff),
        }
    }

    /// Follows the characters' snapshot at `time_ms`: all of them, those
    /// but `skip`, or only `only`; each look readies for the weapon out,
    /// and holds it ready only while it is out (`weapons::armed`).
    pub fn follow(
        &mut self,
        (snapshot, level): (&Snapshot, &sim::Level),
        skip: Option<SimId>,
        only: Option<SimId>,
    ) {
        let fought = fought(Some(snapshot));
        let objects = Instances::in_snapshot(level, snapshot);
        let stances = level.anims().stances();
        let seated: Vec<SimId> = (0..snapshot.seats())
            .filter_map(|seat| snapshot.body(Seat(seat)).map(|body| body.id))
            .collect();
        for entity in &snapshot.entities {
            if skip == Some(entity.id) || only.is_some_and(|only| only != entity.id) {
                continue;
            }
            if let Some(Some(member)) = self.members.get_mut(&entity.id) {
                let (out, held) = (weapon(entity, &objects), sheet(entity, &objects));
                let equipped = held.weapons.iter().any(Option::is_some);
                member.figure.equipped = equipped;
                let armed = entity.fighter.is_none_or(|f| {
                    let targeted = fought.contains(&entity.id);
                    weapons::armed(&f, snapshot.time_ms as f64, targeted, equipped)
                });
                member.figure.ready = ready_anim(out);
                member.figure.wield = held.wield_number();
                member.figure.launcher = held.gear.has(sim::combat::gear::traits::WRIST_LAUNCHER);
                member.figure.breath.wield = stance::wield(out, &held, &self.wields);
                member.figure.follow(
                    &self.rig,
                    (snapshot.time_ms, snapshot.saber_rules, stances),
                    entity,
                    (armed, seated.contains(&entity.id)),
                );
            }
        }
    }

    /// Steers every figure at `ms`, `frame_ms` after the last step; those
    /// in `close` read their torso's motion.
    pub fn steer(
        &mut self,
        history: &[Arc<Snapshot>],
        ms: f64,
        frame_ms: f32,
        close: &[SimId],
        own: Option<&Own<'_>>,
    ) {
        for (&id, member) in &mut self.members {
            if let (Some(member), Some(at)) = (member, steered(history, id, ms, own)) {
                member
                    .figure
                    .steer(&self.rig, &at, ms, frame_ms, close.contains(&id));
            }
        }
    }

    /// The keyframed sounds every JA-animated figure passed since the last
    /// step, at `ms`, by character.
    pub fn keyframed(&mut self, ms: f64) -> Vec<(SimId, Keyed)> {
        let events = self.rig.events.clone();
        let mut out = Vec::new();
        for (&id, member) in &mut self.members {
            let Some(member) = member else { continue };
            if matches!(member.body.look, Look::Clips(_)) {
                continue;
            }
            let mut passed = Vec::new();
            member.figure.keyframed(&self.rig, ms, &events, &mut passed);
            out.extend(passed.into_iter().map(|keyed| (id, keyed.clone())));
        }
        out
    }

    /// The events each figure's KOTOR clip passed as it was drawn since
    /// the last call (`clip_events`), by character.
    pub fn heard(&mut self) -> Vec<(SimId, crate::clip_events::Heard)> {
        self.members
            .iter_mut()
            .filter_map(|(&id, member)| Some((id, member.as_mut()?)))
            .flat_map(|(id, member)| member.heard.drain(..).map(move |heard| (id, heard)))
            .collect()
    }

    /// The characters to pose this frame. The first is the camera's
    /// player. Next come the nearest others to `eye` (game space, metres)
    /// within `POSE_RANGE`, up to `POSED` in all. Next come the
    /// `POSED_IN_VIEW` nearest of the rest within `FAR_RANGE` that `sees`
    /// (the camera's view, in Bevy's space) takes in. A small one among
    /// them is posed only once `SMALL_GAP` has passed since its last
    /// pose. Last comes a `FAR_EVERY`th of the others within `FAR_RANGE`,
    /// in turn. `zoom` is the vertical scale of the camera's projection
    /// (`1 / tan(fov / 2)`), when it has a perspective.
    pub fn nearest(
        &mut self,
        history: &[Arc<Snapshot>],
        ms: f64,
        (eye, sees, zoom): (Vec3, Option<&Frustum>, Option<f32>),
        me: Option<SimId>,
    ) -> Vec<SimId> {
        let mut near: Vec<(f32, SimId, Vec3, f32)> = self
            .members
            .iter()
            .filter(|&(&id, _)| Some(id) != me)
            .filter_map(|(&id, member)| {
                let height = member.as_ref()?.body.height.unwrap_or(STANDING_M);
                let at = Vec3::from_array(origin_at(history, id, ms)?) * UNIT;
                let distance = at.distance(eye);
                (distance <= FAR_RANGE).then_some((distance, id, at, height))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let every_frame = POSED - usize::from(me.is_some());
        let close = near
            .iter()
            .take(every_frame)
            .take_while(|(distance, ..)| *distance <= POSE_RANGE)
            .count();
        let seen = |at: Vec3| {
            sees.is_none_or(|sees| {
                let middle = game_to_bevy() * (at + Vec3::Z);
                let sphere = Sphere {
                    center: middle.into(),
                    radius: IN_VIEW_RADIUS,
                };
                sees.intersects_sphere(&sphere, true)
            })
        };
        let small = |distance: f32, height: f32| {
            zoom.is_some_and(|zoom| height * zoom / (2.0 * distance) < SMALL_SHARE)
        };
        let small_posed = &mut self.small_posed;
        let now = std::time::Instant::now();
        small_posed.retain(|id, _| near.iter().any(|&(_, near, ..)| near == *id));
        let mut in_view = 0;
        self.round = self.round.wrapping_add(1);
        let round = self.round;
        // Turns by id, which stays put as the order by distance shifts.
        let rest = near[close..].iter().filter(|&&(distance, id, at, height)| {
            if in_view < POSED_IN_VIEW && seen(at) {
                if !small(distance, height) {
                    small_posed.remove(&id);
                } else if small_posed
                    .get(&id)
                    .is_some_and(|&was| now.duration_since(was) < SMALL_GAP)
                {
                    return false;
                } else {
                    small_posed.insert(id, now);
                }
                in_view += 1;
                return true;
            }
            (id.0 as usize + round).is_multiple_of(FAR_EVERY)
        });
        let rest: Vec<SimId> = rest.map(|&(_, id, ..)| id).collect();
        me.into_iter()
            .chain(near[..close].iter().map(|&(_, id, ..)| id))
            .chain(rest)
            .collect()
    }

    /// Every drawn character as it stands at `ms`, for dumps: who it is,
    /// how its body is posed, where it stands, what it plays, its health.
    pub fn roster(
        &self,
        history: &[Arc<Snapshot>],
        ms: f64,
        level: &sim::Level,
    ) -> serde_json::Value {
        let Some(newest) = history.last() else {
            return serde_json::Value::Null;
        };
        let objects = Instances::in_snapshot(level, newest);
        let rows = self.members.iter().filter_map(|(&id, member)| {
            let member = member.as_ref()?;
            let (entity, at) = (entity(newest, id)?, blended(history, id, ms)?);
            let who = match entity.kind {
                // An object by its tag; a companion a guest drives as its
                // seat's player too.
                Kind::Object(_, index) => {
                    let tag = objects
                        .tag(index)
                        .map_or_else(|| format!("#{index}"), str::to_string);
                    match entity.fighter.and_then(|fighter| fighter.mind.driver) {
                        Some(seat) => format!("player {} ({tag})", seat.0),
                        None => tag,
                    }
                }
                Kind::Avatar(seat) => format!("player {}", seat.0),
                _ => String::new(),
            };
            let soles = (soles(at.origin) * 100.0)
                .round()
                .to_array()
                .map(|v| v / 100.0);
            let (look, clip) = match &member.body.look {
                Look::Retargeted {
                    gait, acted, shows, ..
                } => (
                    "retargeted",
                    Some(serde_json::json!({
                        "gait": gait.as_ref().map(|gait| gait.facts()),
                        "breath": member.figure.breath.weight(),
                        // How much of each scripted or engine clip shows
                        // over the pose (a mine's disarm, Security).
                        "acted": shows
                            .shown(acted, ms, (member.figure.breath.wield, None))
                            .map(|(clip, _, weight)| (clip.clip().name.clone(), weight))
                            .collect::<Vec<_>>(),
                    })),
                ),
                Look::Ja => ("ja", None),
                Look::Clips(clips) => (
                    "kotor clips",
                    Some(clips.facts(ms, member.figure.breath.wield)),
                ),
            };
            // Every character drawn holds its weapons (none, some).
            let drawn = self.holdings.wielded(id)?.drawn();
            Some(serde_json::json!({
                "id": id.0,
                "who": who,
                "look": look,
                "soles_m": soles,
                "legs_yaw": member.figure.yaw(),
                "head_yaw": member.figure.head.yaw,
                "view": at.view,
                "legs": at.state.legs.anim.name(),
                "torso": at.state.torso.anim.name(),
                "acting": at.fighter.and_then(|f| f.acting).map(|a| {
                    serde_json::json!({ "animation": a.animation, "since": a.since, "ms": a.ms })
                }),
                // A hold on it: what holds it, until when.
                "held": at.fighter.filter(|f| f.force.stunned(ms as u64)).map(|f| {
                    (format!("{:?}", f.force.hold), f.force.stunned_until)
                }),
                // The last hold's kind and end, over or not.
                "last_hold": at.fighter.map(|f| (format!("{:?}", f.force.hold), f.force.stunned_until)),
                // Its movement's base speed (units a second).
                "base_speed": at.state.run_speed,
                "velocity": at.state.velocity,
                // The visuals lasting on it (row, until).
                "visuals": at.fighter.map(|f| {
                    f.effects.visuals(ms as u64).map(|(row, until, _)| (row, until)).collect::<Vec<_>>()
                }),
                "alive": at.alive,
                "hp": entity.fighter.map(|f| f64::from(f.hp) / 100.0),
                "weapons": drawn.shown,
                "blades_lit": drawn.lit,
                "weapons_powered": drawn.powered,
                "saber_away": drawn.away,
                "clip": clip,
                "disguised": member.own.is_some(),
            }))
        });
        serde_json::Value::Array(rows.collect())
    }

    /// Each drawn character's lit blades in hand last frame
    /// (`weapons::Drawn::blades`).
    pub fn blades_in_hand(&self) -> impl Iterator<Item = (SimId, &[(usize, usize)])> {
        self.members.iter().filter_map(|(&id, member)| {
            member.as_ref()?;
            let blades = &self.holdings.wielded(id)?.drawn().blades;
            (!blades.is_empty()).then_some((id, blades.as_slice()))
        })
    }

    /// Weapons shown and blades lit last frame, over every character.
    pub fn arms_drawn(&self) -> (usize, usize) {
        self.members
            .iter()
            .filter(|(_, member)| member.is_some())
            .filter_map(|(&id, _)| self.holdings.wielded(id).map(Wielded::drawn))
            .fold((0, 0), |(shown, lit), drawn| {
                (shown + drawn.shown.len(), lit + drawn.lit)
            })
    }

    /// Notes the speaker and listener of each line of `snapshot`'s
    /// conversations whose DLG hides items (S8), and forgets those of a
    /// conversation that has ended. Nothing to do, and nothing allocated,
    /// while none is under way.
    fn note_unequipped(&mut self, snapshot: Option<&Snapshot>, level: &sim::Level) {
        let talks = snapshot.map_or(&[][..], |snapshot| &snapshot.talk.conversations[..]);
        self.unequipped.retain(|kept| {
            talks
                .iter()
                .any(|talk| talk.owner == kept.owner && talk.dialog == kept.dialog)
        });
        let (Some(snapshot), Some(scripts)) = (snapshot, level.scripts()) else {
            return;
        };
        for talk in talks {
            let Some(dlg) = scripts.dialogue(&talk.dialog) else {
                continue;
            };
            // `UnequipHItem` hides the head item, not the weapons (TSL's
            // executable hides a creature's equipped head item for it):
            // `headgear::TalkHides`.
            if !dlg.unequip_items {
                continue;
            }
            let hide = Hide::All;
            let LinePair {
                speaker, listener, ..
            } = line_pair(snapshot, talk);
            for who in [speaker, listener] {
                let noted = self.unequipped.iter().any(|kept| {
                    kept.who == who && kept.owner == talk.owner && kept.dialog == talk.dialog
                });
                if !noted {
                    self.unequipped.push(Unequipped {
                        owner: talk.owner,
                        dialog: talk.dialog.clone(),
                        who,
                        hide,
                    });
                }
            }
        }
    }

    /// Notes each conversation's speaker in `snapshot` (but an animated
    /// cut's or a staged one's, its bodies on their stunt clips): since its
    /// first line of those it says running, so its talk loops on through
    /// them; forgets those of a conversation that has ended.
    fn note_talking(&mut self, snapshot: Option<&Snapshot>, level: &sim::Level) {
        let talks = snapshot.map_or(&[][..], |snapshot| &snapshot.talk.conversations[..]);
        let scripts = level.scripts();
        let talking = talks.iter().filter_map(|talk| {
            let dlg = scripts?.dialogue(&talk.dialog)?;
            let who = SimId(talk.speaker.0);
            if dlg.staged() || talk.speaker == ObjectId::INVALID {
                return None;
            }
            let since = self
                .talking
                .iter()
                .find(|kept| kept.id == talk.id && kept.who == who)
                .map_or(talk.shown_ms, |kept| kept.since);
            Some(Talking {
                id: talk.id,
                who,
                since,
            })
        });
        self.talking = talking.collect();
    }

    /// Places every drawn character at `ms`, poses those in `posing` (a
    /// gripping hand reaching for its blade in `reach`, as the sim has it,
    /// unless `quirks`), and draws what they hold. A character's death is a
    /// ragdoll (`ragdolls`) falling in `level`: posed while it falls, left
    /// as it lies once still. What the camera's player's retarget gave,
    /// for its trace.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        (history, level): (&[Arc<Snapshot>], &sim::Level),
        ms: f64,
        (posing, reach): (&[SimId], &Reaching),
        me: Option<SimId>,
        own: Option<&Own<'_>>,
        transforms: &mut Query<&mut Transform>,
        props: &mut Props,
    ) -> Option<Posed> {
        let started = std::time::Instant::now();
        let mut arms = std::time::Duration::ZERO;
        let mut mine = None;
        // Whom the living are fighting: they have their weapons out.
        let newest = history.last().map(|newest| &**newest);
        let fought = fought(newest);
        let held = newest.is_some_and(Snapshot::holds_world);
        let rules = newest.map_or(SaberRules::Ja, |newest| newest.saber_rules);
        let states = newest.and_then(|newest| level.states_in(newest));
        self.note_unequipped(newest, level);
        self.note_talking(newest, level);
        let looks = history
            .last()
            .map_or_else(Vec::new, |newest| looks(newest, level));
        if self.floor_states != states {
            for member in self.members.values_mut().flatten() {
                member.floor = None;
            }
            self.floor_states.clone_from(&states);
        }
        self.body_clips.clear();
        let mut placing = Vec::new();
        for (&id, member) in &mut self.members {
            let Some(member) = member else {
                continue;
            };
            let at = match blended_for(history, id, ms, own) {
                Some(at) => {
                    member.gone = at.dead.then_some(at);
                    at
                }
                // Taken away dead: its death plays on where it lay.
                None => match member.gone.filter(|_| member.playing_death(ms)) {
                    Some(at) => at,
                    None => continue,
                },
            };
            let blasted = blasted(&at, ms);
            if at.alive && blasted.is_none() {
                self.ragdolls.rise(id);
            }
            let still = self.ragdolls.still(id);
            let ms = match own {
                Some(own) if own.id == id => own.ms,
                _ => ms,
            };
            let armed = at.fighter.is_none_or(|f| {
                weapons::armed(&f, ms, fought.contains(&id), member.figure.equipped)
            });
            let reach = reach
                .get(&id)
                .filter(|_| !self.rig.quirks)
                .map_or(&[][..], Vec::as_slice);
            // Only the dead and the blasted fall as ragdolls: the downed
            // must get up, and one dying in place lies as its clip leaves it.
            let in_place = dying_in_place(&at);
            let falls = (at.dead || blasted.is_some()) && member.body.doll.is_some() && !in_place;
            let death_plays = member.playing_death(ms);
            // Holstered guns drawn or put away as the body readies them
            // (`draw_anim`): only a body that carries them.
            let holsters = self
                .holdings
                .wielded(id)
                .map_or([None; 2], Wielded::holsters);
            let readied = armed && at.alive;
            member.figure.readied = readied;
            let drawn = if holsters.iter().any(Option::is_some) {
                Some(member.figure.drawing.step(readied, ms))
            } else {
                // No holstered gun (another weapon, or none): the track
                // starts over, holstered, so nothing it held lingers (a
                // torso kept up) and a pistol taken out next is drawn.
                member.figure.drawing = crate::draw_anim::Drawing::at(false);
                None
            };
            let look = looks
                .iter()
                .filter(|_| !still)
                .find(|look| look.0 == id)
                .and_then(|&(_, other, arc)| {
                    Some((self::soles(origin_for(history, other, ms, own)?), arc))
                });
            // The bolts that struck it lately, while it lives.
            let flinches: Vec<(Location, Vec3, f32)> = (self.bolt_hits.iter())
                .filter(|hit| !still && hit.victim == id && at.alive)
                .map(|hit| (hit.location, hit.way, hit.ms as f32))
                .collect();
            // The own gun seen from behind aims at the point under the
            // crosshair, found only while a gun is out.
            let seen_from_behind = Some(id) == me && self.eye.is_none();
            let sighted = (!still && seen_from_behind && armed && at.alive)
                .then_some(newest)
                .flatten()
                .filter(|_| self.holdings.wielded(id).is_some())
                .map(|newest| {
                    let mut player = at.state;
                    player.view = at.view;
                    player.origin = at.origin;
                    crate::aim_pose::sighted((level, states.as_deref()), newest, id, &player)
                });
            placing.push(Placing {
                id,
                member,
                at,
                blasted,
                ms,
                still,
                falls,
                armed,
                drawn,
                flinches,
                sighted,
                full: posing.contains(&id) || falls || in_place || death_plays,
                reach,
                look,
                talking: talking(&self.talking, id),
                touches: Vec::new(),
                posed: None,
            });
        }
        let stage = Stage {
            level,
            states: states.as_deref(),
            held,
            rules,
            me,
            eye: self.eye,
            holdings: &self.holdings,
        };
        // A grapple finisher's grabbers are posed after everyone else: their
        // hands reach for where their victims are drawn (`grapple_view`).
        let mut moving: Vec<&mut Placing> = (placing.iter_mut())
            .filter(|p| !p.still && !p.grabs())
            .collect();
        let parallel = moving.iter().filter(|p| p.full).count() >= PARALLEL_FROM;
        match ComputeTaskPool::try_get().filter(|_| parallel) {
            Some(pool) => {
                let world = &*transforms;
                let size = moving.len().div_ceil(pool.thread_num() * 4).max(1);
                let written = pool.scope(|scope| {
                    for chunk in moving.chunks_mut(size) {
                        let stage = &stage;
                        scope.spawn(async move {
                            (chunk.iter_mut())
                                .map(|placing| {
                                    let mut staged = Staged::new(world);
                                    placing.place(stage, &mut staged);
                                    staged.written()
                                })
                                .collect::<Vec<_>>()
                        });
                    }
                });
                for written in written.into_iter().flatten() {
                    written.apply(transforms);
                }
            }
            None => {
                for placing in moving {
                    placing.place(&stage, transforms);
                }
            }
        }
        let victims: BTreeMap<SimId, (SimId, crate::grapple_view::Held)> = (placing.iter())
            .filter(|p| !p.still)
            .filter_map(|p| {
                let grapple = p.grapple().filter(|grapple| !grapple.grabber)?;
                Some((p.id, (grapple.partner, p.member.grapple_nodes(transforms)?)))
            })
            .collect();
        for grabber in placing.iter_mut().filter(|p| !p.still && p.grabs()) {
            grabber.touches = grabber.reach_for(&victims, &self.rig);
            grabber.place(&stage, transforms);
        }
        let mut dying = Vec::new();
        for Placing {
            id,
            member,
            at,
            blasted,
            ms,
            still,
            falls,
            armed,
            drawn,
            sighted,
            posed,
            ..
        } in placing
        {
            let moved = posed.as_ref().and_then(|posed| posed.root);
            if !still {
                if Some(id) == me {
                    self.sight = sighted.and_then(|point| {
                        let wielded = self.holdings.wielded(id)?;
                        let (gun, shape) = wielded.held_ups()[0]?;
                        let support = gun
                            .and_then(|gun| weapons::handling(gun.blaster).support)
                            .filter(|_| !(wielded.pair() || gun.is_some_and(|g| g.one_handed)))
                            .map(|share| shape.foregrip(share));
                        let firing =
                            firing_of(&member.figure, Some(wielded), (at.alive, false), ms);
                        let push = firing.map_or_else(Default::default, |firing| {
                            crate::recoil::push(&firing, 0, ms)
                        });
                        let raise = member.figure.raise_share(ms);
                        sight_of(
                            &member.body,
                            (shape, support),
                            (point, raise, push),
                            transforms,
                        )
                    });
                }
                if let Some(playing) = &mut member.body.playing
                    && !playing.is_empty()
                {
                    let playing = std::mem::take(playing);
                    self.body_clips.insert(member.body.model, playing);
                }
                if Some(id) == me {
                    mine = posed;
                }
            }
            if falls
                && !still
                && let Some(one) =
                    as_drawn_dead(&self.rig, (id, member), (at, blasted), moved, transforms)
            {
                dying.push(one);
            }
            if let (Some(fighter), Some(wielded)) = (&at.fighter, self.holdings.wielded_mut(id)) {
                let moment = weapons::Moment {
                    fighter,
                    alive: at.alive,
                    ms,
                    origin: at.origin,
                    thrown_at: at.thrown_at,
                    torso: at.state.torso.anim.name(),
                    armed,
                    hidden: hidden(&self.unequipped, id),
                    guns_in_hand: drawn.map(|drawn| drawn.in_hand),
                };
                let drawing = std::time::Instant::now();
                wielded.draw(&moment, transforms, props);
                arms += drawing.elapsed();
            }
        }
        self.ragdolls
            .update((&dying, &self.bolt_hits), (level, history), ms);
        for (id, sever) in self.ragdolls.severed.drain(..) {
            if let Some(Some(member)) = self.members.get(&id) {
                self.cuts.push((member.body.nodes.clone(), sever));
            }
        }
        let fallen = self.members.iter().filter_map(|(&id, member)| {
            let body = &member.as_ref()?.body;
            Some((id, body.root, body.nodes.as_slice()))
        });
        self.ragdolls.draw(fallen, ms, transforms);
        self.arms_ms = arms.as_secs_f64() * 1000.0;
        self.posed = posing.len();
        self.pose_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.totals.add(self.posed, self.pose_ms);
        mine
    }
}

/// With fewer, waking the threads and copying each character's transforms
/// costs about as much time as it saves.
const PARALLEL_FROM: usize = 8;

/// Posing one character never reads what posing another writes, so the
/// characters can be posed in any order, or in parallel (`Cast::draw`).
struct Placing<'a> {
    id: SimId,
    member: &'a mut Member,
    at: Blended,
    blasted: Option<u64>,
    ms: f64,
    still: bool,
    falls: bool,
    armed: bool,
    drawn: Option<DrawPose>,
    flinches: Vec<(Location, Vec3, f32)>,
    sighted: Option<Vec3>,
    full: bool,
    reach: &'a [(Blade, Hand)],
    look: Option<(Vec3, f32)>,
    talking: Option<Acting>,
    /// A grapple finisher's grabber: where its hands hold its victim.
    touches: Vec<GrappleTouch>,
    posed: Option<Posed>,
}

struct Stage<'a> {
    level: &'a sim::Level,
    states: Option<&'a [u8]>,
    held: bool,
    rules: SaberRules,
    me: Option<SimId>,
    eye: Option<Vec3>,
    holdings: &'a Holdings,
}

impl Placing<'_> {
    /// The grab it is in, holding or held.
    fn grapple(&self) -> Option<sim::combat::grab::Grapple> {
        self.at.fighter.as_ref().and_then(|fighter| fighter.grapple)
    }

    /// Whether it holds another in a grapple finisher.
    fn grabs(&self) -> bool {
        self.grapple().is_some_and(|grapple| grapple.grabber)
    }

    /// Where a grabber's hands hold its victim, drawn already (`victims`:
    /// each one's grabber and where its nodes are). None for a body JA's
    /// clips do not drive, or whose victim is not drawn.
    fn reach_for(
        &self,
        victims: &BTreeMap<SimId, (SimId, crate::grapple_view::Held)>,
        rig: &Rig,
    ) -> Vec<GrappleTouch> {
        let found = self
            .grapple()
            .zip(self.member.ja_scale())
            .and_then(|(grapple, scale)| {
                let (partner, held) = victims.get(&grapple.partner)?;
                let t = ((self.ms - grapple.since as f64) / 1000.0) as f32;
                (*partner == self.id).then(|| {
                    crate::grapple_view::touches(
                        crate::grapple_view::contacts(rig.gla, &rig.sequences),
                        &grapple,
                        t,
                        (self.at.state.view[1], scale),
                        held,
                    )
                })
            });
        found.unwrap_or_default()
    }

    fn place(&mut self, stage: &Stage, transforms: &mut impl Transforms) {
        let (id, at, member) = (self.id, &self.at, &mut *self.member);
        let soles = soles(at.origin);
        // A body that leans with the floor, or stands on it, finds
        // it under its middle; one standing upright is drawn on a
        // slope there (`ground_lean::on_slope`), the leaning one as
        // its lean has it.
        let leans = matches!(&member.body.look, Look::Clips(clips) if clips.lean.is_some());
        let floor = (leans || at.state.ground.is_some())
            .then(|| {
                let from = (Vec3::from_array(at.origin) * UNIT, soles);
                match member.floor {
                    Some((found, floor)) if found == from => floor,
                    _ => {
                        let floor = ground_lean::floor_under(stage.level, stage.states, from);
                        member.floor = Some((from, floor));
                        floor
                    }
                }
            })
            .flatten();
        let stands = if leans {
            soles
        } else {
            ground_lean::on_slope(floor, soles)
        };
        let own = Some(id) == stage.me;
        let eye = stage.eye.filter(|_| own);
        self.posed = place(
            member,
            at,
            (stands, self.ms, eye, floor),
            (self.full, self.armed, self.reach, own),
            (stage.held, self.look, self.drawn, stage.rules),
            (self.talking, &self.flinches, &self.touches),
            (stage.holdings.wielded(id), self.sighted),
            transforms,
        );
    }
}

/// A conversation's line as it is filmed and faced (S2, S6): its speaker and
/// the one it is said to (`Conversation::listener_now`); while the replies
/// are offered (`offered`), the player and the one who spoke. The player is
/// its listener where that is a guest seat's body (R2: a guest's companion
/// in that guest's own conversation, though the scripts' PC is the host's
/// character), else its PC (the host's character, also while the host leads
/// through a companion).
pub struct LinePair {
    pub speaker: SimId,
    pub listener: SimId,
    pub player: SimId,
    pub offered: bool,
}

/// `talk`'s line in `snapshot` (`LinePair`).
pub fn line_pair(snapshot: &Snapshot, talk: &Conversation) -> LinePair {
    let offered = talk.until_ms == u64::MAX && !talk.replies.is_empty() && !talk.auto;
    let guest = (1..snapshot.seats()).any(|guest| {
        snapshot
            .body(Seat(guest))
            .is_some_and(|body| body.id.0 == talk.listener.0)
    });
    let player = SimId(if guest { talk.listener.0 } else { talk.pc.0 });
    let listener = match talk.listener_now {
        ObjectId::INVALID => player,
        listener => SimId(listener.0),
    };
    let speaker = if offered {
        player
    } else {
        SimId(talk.speaker.0)
    };
    LinePair {
        speaker,
        listener,
        player,
        offered,
    }
}

/// Who looks at whom in the conversations under way (S7), whether or not
/// one holds the world (a body S2 turns short of its listener leaves the
/// rest to its head), and how far the looker's head turns (degrees, its
/// `head_arc_h`; a player's by its persona's appearance): each line's pair
/// (`line_pair`) at each other, but in an animated cut or a staged
/// conversation (its bodies on their stunt clips) and a body on the
/// head-follow lock list. A head of no arc does not turn [verify]. None
/// with no conversation under way (free play).
fn looks(snapshot: &Snapshot, level: &sim::Level) -> Vec<(SimId, SimId, f32)> {
    let Some(scripts) = level
        .scripts()
        .filter(|_| !snapshot.talk.conversations.is_empty())
    else {
        return Vec::new();
    };
    let objects = Instances::in_snapshot(level, snapshot);
    let arc = |id: SimId| {
        let entity = entity(snapshot, id)?;
        match entity.kind {
            Kind::Avatar(seat) => {
                let (row, _) = sim::persona::dressed(snapshot, seat)?;
                Some(scripts.head_arc(row))
            }
            _ => Some(objects.of_entity(entity)?.body?.head_arc_h),
        }
    };
    let mut looks = Vec::new();
    for talk in &snapshot.talk.conversations {
        let staged = scripts
            .dialogue(&talk.dialog)
            .is_none_or(|dlg| dlg.animated_cut || dlg.staged());
        if staged {
            continue;
        }
        let LinePair {
            speaker, listener, ..
        } = line_pair(snapshot, talk);
        for (who, other) in [(speaker, listener), (listener, speaker)] {
            if who == other || talk.lock_head.contains(&ObjectId(who.0)) {
                continue;
            }
            if let Some(arc) = arc(who).filter(|arc| *arc > 0.0) {
                looks.push((who, other, arc));
            }
        }
    }
    looks
}

/// What `who` plays as a conversation's speaker (`Cast::talking`): its
/// `tlknorm` looped from its first line of those it says running, as
/// reone's speaker talks (`Creature::startTalking`) until another speaks
/// or the conversation ends, the replies offered between included. A
/// listener plays only what its line's `AnimList` gives it [verify].
fn talking(talking: &[Talking], who: SimId) -> Option<Acting> {
    let talk = talking.iter().find(|talk| talk.who == who)?;
    Some(Acting {
        animation: acting::LOOPING_TALK_NORMAL,
        since: talk.since,
        ms: u32::MAX,
    })
}

/// What the conversations under way hide of `who`'s item models.
fn hidden(unequipped: &[Unequipped], who: SimId) -> Hide {
    unequipped
        .iter()
        .filter(|kept| kept.who == who)
        .map(|kept| kept.hide)
        .max()
        .unwrap_or(Hide::None)
}

/// The clip a held `fighter` plays at `ms` while it is held
/// (`acting::of_state`), of the highest state on it: C11's hold (begun its
/// round back from its end) or a state's effect, a cutscene's among them.
/// A body with JA's clips (`ja_body`) covering its ears plays JA's own
/// clip for it (the sim's), so the hold draws nothing over it.
fn held_clip(fighter: &Fighter, ms: f64, ja_body: bool) -> Option<Acting> {
    if !fighter.alive() {
        return None;
    }
    let force = &fighter.force;
    let ears = ja_body && force.covering_ears(ms as u64).is_some();
    let hold = (ms < force.stunned_until as f64 && !ears).then(|| {
        let since = force
            .stunned_until
            .saturating_sub(sim::combat::force::ja::STUN_MS);
        (force.hold.state(), since, force.stunned_until)
    });
    let effect = fighter
        .effects
        .top_state(ms as u64)
        .map(|lasting| (lasting.of, lasting.at, lasting.until));
    let (state, since, until) = hold
        .into_iter()
        .chain(effect)
        .max_by_key(|&(state, ..)| state)?;
    Some(Acting {
        animation: acting::of_state(state)?,
        since,
        ms: (until - since).min(u64::from(u32::MAX)) as u32,
    })
}

/// What `fighter` plays at `ms`, as `place` draws it: a hold's clip, else a
/// script's animation. One played out is over for a body on the move
/// (`moving`, faster than `STILL_SPEED`): a round's would hold its ready
/// stance over a run (`Shows::shown`) until the next line clears it. A body
/// with JA's clips is `ja_body` (`held_clip`).
pub fn drawn_acting(fighter: &Fighter, moving: bool, ms: f64, ja_body: bool) -> Option<Acting> {
    held_clip(fighter, ms, ja_body).or(fighter
        .acting
        .filter(|acting| !moving || acting.playing(ms as u64)))
}

/// Whether the character at `at` is dead and dying in place
/// (`sim::combat::death::in_place`): its own dying clip plays where it
/// stands and its last frame holds (`acting::Shows`), with no ragdoll.
fn dying_in_place(at: &Blended) -> bool {
    at.dead
        && at
            .fighter
            .as_ref()
            .and_then(|fighter| fighter.acting)
            .is_some_and(|acting| acting.animation == sim::combat::death::DIE)
}

/// A character dead at `at` as drawn this frame, for its ragdoll: the root
/// and pose its death animation put it in (a retarget having `moved` its
/// root node by an offset), when it died, how long the animation lasts.
fn as_drawn_dead(
    rig: &Rig,
    (id, member): (SimId, &Member),
    (at, blasted): (Blended, Option<u64>),
    moved: Option<(usize, Vec3)>,
    transforms: &Query<&mut Transform>,
) -> Option<Dying> {
    let doll = member.body.doll.clone()?;
    let (lengths, length_ms) = match &member.body.look {
        Look::Retargeted { rig: body, .. } => {
            let moved = moved.and_then(|(node, offset)| {
                Some((node, body.rest.get(node)?.1 + body.animation_scale * offset))
            });
            let length_ms = rig
                .sequences
                .get(at.state.legs.anim.name())
                .filter(|s| s.frames_per_second > 0)
                .map(|s| f64::from(s.frame_count) * 1000.0 / f64::from(s.frames_per_second));
            (Lengths::Rest(moved), length_ms)
        }
        Look::Clips(clips) => (Lengths::Drawn, clips.die_ms()),
        Look::Ja => return None,
    };
    let root = transforms.get(member.body.root).ok()?;
    // A blasted body falls from the blast, limp at once: no clip of its own
    // to hold to.
    let (died_ms, length_ms) = match blasted {
        Some(blasted) => (blasted, Some(BLASTED_MS)),
        None => (member.figure.died()?, length_ms),
    };
    Some(Dying {
        id,
        pose: ragdolls::drawn_pose(&doll.rest, &member.body.nodes, transforms, lengths),
        doll,
        root: ragdoll::Frame::new(root.translation, root.rotation),
        at,
        died_ms: died_ms as f64,
        length_ms,
    })
}

/// How long (ms) a blasted body's muscles hold before it goes limp.
const BLASTED_MS: f64 = 150.0;

/// When the script's push (a blast, `Fighter::blasted_ms`) threw the body at
/// `at`, while `ms` is within JA's knockdown from it: it falls as a ragdoll
/// from then, the throw's velocity carried, not knocked down by JA's clip,
/// and a death meanwhile (the scene's that follows its blast: TSL's Sith
/// troopers on the Ravager, pushed from `wp_explosion` and killed 0.9 s on)
/// lets it lie as it fell; alive past the knockdown, it rises.
fn blasted(at: &Blended, ms: f64) -> Option<u64> {
    let blasted = at.fighter.as_ref()?.blasted_ms;
    let since = (ms as u64).checked_sub(blasted)?;
    (blasted > 0 && since < sim::combat::force::ja::KNOCKDOWN_MS).then_some(blasted)
}

/// Shows or hides `root` (a body's), if it is not so already.
fn show(visibility: &mut Query<&mut Visibility>, root: Entity, shown: bool) {
    let wanted = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if let Ok(mut now) = visibility.get_mut(root)
        && *now != wanted
    {
        *now = wanted;
    }
}

/// Changes an entity's transform, if it has one.
fn edit(transforms: &mut impl Transforms, entity: Entity, change: impl FnOnce(&mut Transform)) {
    if let Some(transform) = transforms.write(entity) {
        change(transform);
    }
}

/// Which of `ir`'s nodes a body leaves where the last clip keying them put
/// them (`at_rest`): those its standing clip (`pause`) does not key. KOTOR
/// writes only the nodes the playing clip keys (reone, xoreos), so a node
/// its standing clip keys is set again whenever the body stands, and any
/// other keeps the pose the last clip gave it: a robe's hood thrown back by
/// `offhood` stays back until `onhood`. No standing clip: none.
pub fn held_nodes(ir: &ModelIr, pause: Option<&AnimationIr>) -> Vec<bool> {
    pause.map_or_else(Vec::new, |pause| {
        pause.bind(ir).iter().map(Option::is_none).collect()
    })
}

/// Puts a body's nodes back at `rest` before it is posed afresh, but those
/// `held` (`held_nodes`): what neither the frame's JA pose nor its clips
/// key stands at rest, as the standing clip KOTOR plays under them would
/// set it, never as a clip since ended left it (a lying clip's turn of the
/// root dummy, which would tilt the whole body once it stands again). Only
/// a node that moved is written.
pub(crate) fn at_rest(
    (nodes, rest, held): (&[Entity], &Rest, &[bool]),
    transforms: &mut impl Transforms,
) {
    for (index, (&entity, &(_, translation, rotation))) in nodes.iter().zip(rest).enumerate() {
        if held.get(index).copied().unwrap_or(false) {
            continue;
        }
        // Read first, so a node is marked changed only when it is written.
        if transforms
            .read(entity)
            .is_some_and(|t| t.translation != translation || t.rotation != rotation)
            && let Some(t) = transforms.write(entity)
        {
            t.translation = translation;
            t.rotation = rotation;
        }
    }
}

/// Those a living fighter in `snapshot` fights (`weapons::armed`'s
/// `targeted`).
fn fought(snapshot: Option<&Snapshot>) -> std::collections::BTreeSet<SimId> {
    snapshot
        .into_iter()
        .flat_map(|snapshot| &snapshot.entities)
        .filter_map(|e| e.fighter.filter(Fighter::alive)?.target)
        .collect()
}

/// What the firing motion (`recoil`) needs of `figure` this frame, from
/// its guns in hand (`gun`), whether its holder is `alive`, and whether the
/// player looks through its eyes (`looked_through`), at `ms`. `None` for
/// what shoots nothing (a stun baton) or no gun at all.
fn firing_of(
    figure: &Figure,
    gun: Option<&Wielded>,
    (alive, looked_through): (bool, bool),
    ms: f64,
) -> Option<crate::recoil::Firing> {
    let wielded = gun?;
    let blasters = wielded
        .held_ups()
        .map(|held| held.and_then(|(gun, _)| gun).map(|gun| gun.blaster));
    blasters[0]?;
    Some(crate::recoil::Firing {
        wield: Some(aim_pose::wield_of(figure.breath.wield)),
        blasters,
        shots: hand_shots(figure, wielded.pair()),
        streak: figure.streak,
        raise: figure.raise_share(ms),
        looked_through,
        alive,
    })
}

/// Adds the firing motion's turns (`recoil::turns`) over the body's pose,
/// whatever it shows, after the arms hold the guns where the shots pushed
/// them (`gun_aim`). A pistol's forearm and upper arm follow its kick, and
/// sustained fire leans the chest in. A rifle's chest turns before the arms
/// reach the gun (`recoil::Push::chest_deg`). `gun` is the body's guns in
/// hand, `alive` whether its holder lives, and `looked_through` whether the
/// player looks through its eyes. A turn's pitch is about the body's right and its yaw
/// about its up, both taken from the way the legs face
/// (`recoil::rotation`).
fn recoil(
    figure: &Figure,
    transforms: &mut impl Transforms,
    (nodes, rig): (&[Entity], &BodyRig),
    (gun, alive, looked_through): (Option<&Wielded>, bool, bool),
    (ms, to_model): (f64, Quat),
) {
    let Some(firing) = firing_of(figure, gun, (alive, looked_through), ms) else {
        return;
    };
    let turns = crate::recoil::turns(&firing, ms);
    if turns.is_empty() {
        return;
    }
    let legs = figure.yaw().unwrap_or(0.0).to_radians();
    let right = to_model * Vec3::new(legs.sin(), -legs.cos(), 0.0);
    let up = to_model * Vec3::Z;
    let model = rig.model();
    for turn in turns {
        let Some(node) = (0..model.nodes.len())
            .find(|&node| model.nodes[node].name.eq_ignore_ascii_case(turn.node))
        else {
            continue;
        };
        let rotation = crate::recoil::rotation(&turn, right, up);
        turn_in_model(transforms, (nodes, &rig.rest), node, rotation);
    }
}

/// Each hand's last shot and the one before, as the motion of `pair` or of
/// one gun reads them (`recoil::Shots::held`).
fn hand_shots(figure: &Figure, pair: bool) -> crate::recoil::Shots {
    crate::recoil::Shots::held(figure.shot_ms, figure.shot_before_ms, pair)
}

/// The gun `wielded` has out held up and aimed down its shots' way
/// (`asset_ja::rig::Aim`, KOTOR model space by `to_model` from the body at
/// `soles`): eased in over `AIM_EASE_MS` as the torso takes its ready or
/// shot animation with the gun out (`armed`) and its holder alive, and out
/// as the gun goes away (`HOLSTER_BLEND_MS`) or anything else plays; fully
/// aimed for `SHOT_AIM_MS` after a shot; pushed back and kicked up by its
/// last shot, with the shoulder and a rifle's chest giving way, less far
/// the further a rifle is raised (`recoil::push`); held up before
/// the `eye` (`Cast::eye`) at `HOLD`. A player's gun seen from behind aims
/// at the point under its crosshair (`sighted`, units, game space:
/// `aim_pose::sighted`), where its bolts go.
/// None without a gun, or once eased out.
fn gun_aim(
    figure: &mut Figure,
    at: &Blended,
    (wielded, armed, drawn): (Option<&Wielded>, bool, Option<DrawPose>),
    ms: f64,
    (to_model, soles, eye, sighted): (Quat, Vec3, Option<Vec3>, Option<Vec3>),
) -> Vec<Aim> {
    // What shoots nothing (a stun baton) is held up only before the eye.
    let guns = wielded.map_or([None, None], Wielded::held_ups);
    let guns = guns.map(|gun| gun.filter(|(gun, _)| gun.is_some() || eye.is_some()));
    let pair = wielded.is_some_and(Wielded::pair);
    let up = armed && at.alive && guns[0].is_some();
    // A gun ready or shooting; a stun baton's torso, following the legs
    // but for a punch (`motion::weapon`), between punches.
    let torso = at.state.torso.anim;
    let holding = up
        && match guns[0] {
            Some((Some(_), _)) => matches!(
                torso,
                Anim::TORSO_WEAPONREADY2
                    | Anim::TORSO_WEAPONREADY3
                    | Anim::BOTH_ATTACK2
                    | Anim::BOTH_ATTACK3
            ),
            _ => torso == at.state.legs.anim,
        };
    let frame_ms = figure.aim_ms.map_or(0.0, |was| (ms - was).max(0.0)) as f32;
    figure.aim_ms = Some(ms);
    let (target, ease_ms) = if holding {
        (1.0, AIM_EASE_MS)
    } else {
        (0.0, HOLSTER_BLEND_MS / 3.0)
    };
    let shots = hand_shots(figure, pair).last;
    let shooting = shots
        .iter()
        .flatten()
        .any(|shot| (0.0..SHOT_AIM_MS).contains(&(ms - shot)));
    let firing = firing_of(figure, wielded, (at.alive, eye.is_some()), ms);
    let raised = figure.raise_share(ms);
    let mut ps = at.state;
    ps.view = at.view;
    ps.origin = at.origin;
    let [forward, right, _] = view_axes(at);
    // A draw or a holster under way leads the arms (`draw_anim`): aimed
    // as far as it has them, their hands on the holstered guns as far.
    let holsters = wielded.map_or([None; 2], Wielded::holsters);
    let moving = drawn.filter(|_| !figure.drawing.settled());
    let mut aims = Vec::new();
    for (hand, gun) in guns.into_iter().enumerate() {
        let aim = &mut figure.aim[hand];
        *aim += (target - *aim) * (1.0 - (-frame_ms / ease_ms).exp());
        if up && shooting {
            *aim = 1.0;
        }
        if let Some(drawn) = moving {
            *aim = drawn.aim;
        }
        let Some((gun, shape)) = gun else {
            continue;
        };
        let blaster = gun.map(|gun| gun.blaster);
        let stow = moving
            .zip(holsters[hand])
            .map(|(drawn, (node, place))| Stowed {
                node,
                place: (place.rotation, place.translation),
                share: drawn.stow,
            });
        if *aim < 1e-3 && stow.is_none_or(|stow| stow.share < 1e-3) {
            continue;
        }
        let handling = blaster.map_or(weapons::Handling::STILL, weapons::handling);
        // The other hand holds a gun only where both hands do: not a
        // pistol, nor either gun of a pair.
        let hand_alone = pair || gun.is_some_and(|gun| gun.one_handed);
        let push = firing.map_or_else(Default::default, |firing| {
            crate::recoil::push(&firing, hand, ms)
        });
        // The barrel aims at the shot's way `CONVERGE` units out, where the
        // bolt drawn from the muzzle meets it (`combat_fx::Path`); what
        // shoots nothing, along the view. A pair's left gun from its own
        // muzzle (`sim::hand_muzzle`), held up mirrored before the eye. A
        // player's gun seen from behind aims at the point under its
        // crosshair, so the barrel meets the crosshair's line there rather
        // than running beside it.
        let side = if hand == 1 { Side::Left } else { Side::Right };
        let muzzle = Vec3::from_array(match blaster {
            Some(blaster) => sim::hand_muzzle(
                &ps,
                sim::combat::weapons::primary(blaster).muzzle,
                hand as u8,
            ),
            None => sim::muzzle(&ps, [0.0; 3]),
        });
        let target = sighted
            .filter(|_| eye.is_none() && blaster.is_some())
            .unwrap_or(muzzle + forward * crate::combat_fx::CONVERGE);
        let hold = eye.map(|eye| {
            let [ahead, aside, down] = HOLD;
            let aside = if side == Side::Left { -aside } else { aside };
            to_model * (before_eyes(eyes(at, eye), (forward, right), [ahead, aside, down]) - soles)
        });
        aims.push(Aim {
            side,
            target: to_model * (target * UNIT - soles),
            muzzle: shape.muzzle,
            barrel: shape.barrel,
            up: shape.up,
            support: handling
                .support
                .filter(|_| !hand_alone)
                .map(|share| shape.foregrip(share)),
            // Both hands on a gun seen from outside: its stock at the
            // shoulder.
            butt: (handling.support.is_some() && !hand_alone && eye.is_none())
                .then_some((shape.seat, raised)),
            forward: handling.forward,
            kick: (push.up_deg.to_radians(), push.back_m),
            shoulder: push.shoulder_deg.to_radians(),
            chest: push.chest_deg.to_radians(),
            weight: *aim,
            hold,
            stow,
        });
    }
    aims
}

/// How the gun in `body`'s right hand (its shape `shape`) and its head line
/// up with `point` (units, game space) as `transforms` pose them now, the
/// gun raised as far as `raise` says and pushed by its last shot as `push`
/// says (`aim_pose::Sight`). None for a body not retargeted from JA's
/// skeleton.
fn sight_of(
    body: &Body,
    (shape, support): (&crate::weapons::GunShape, Option<Vec3>),
    (point, raise, push): (Vec3, f32, crate::recoil::Push),
    transforms: &Query<&mut Transform>,
) -> Option<crate::aim_pose::Sight> {
    let Look::Retargeted { rig, .. } = &body.look else {
        return None;
    };
    let model = rig.model();
    let find = |name: &str| {
        (0..model.nodes.len()).find(|&n| model.nodes[n].name.eq_ignore_ascii_case(name))
    };
    let hand = find("rhand")?;
    let root = *transforms.get(body.root).ok()?;
    let placed = |node: usize| placed_in_model(transforms, (&body.nodes, &rig.rest), node);
    let in_model = placed(hand)?;
    let gun = root.mul_transform(in_model);
    // The stock and the support hand, in the model's space (metres).
    let at = |name: &str| Some(placed(find(name)?)?.translation);
    let stock = in_model.transform_point(shape.butt);
    let muzzle_in_model = in_model.transform_point(shape.muzzle);
    let ahead = ((muzzle_in_model - stock) * Vec3::new(1.0, 1.0, 0.0)).normalize_or_zero();
    let right = ahead.cross(Vec3::Z);
    let stock_m = at("rbicep_g").map(|shoulder| {
        let off = stock - shoulder;
        [off.dot(ahead), off.z, off.dot(right)]
    });
    let aside_m = at("rbicep_g").zip(at("lbicep_g")).map(|(r, l)| {
        let middle = (r + l) / 2.0;
        [
            (stock - middle).dot(right),
            (muzzle_in_model - middle).dot(right),
        ]
    });
    let mean = |names: [&str; 4]| -> Option<Vec3> {
        let mut sum = Vec3::ZERO;
        for name in names {
            sum += at(name)?;
        }
        Some(sum / 4.0)
    };
    let support_gap_m = support.and_then(|grip| {
        let knuckles = mean(["lafngrb_g", "lbfngrb_g", "lcfngrb_g", "ldfngrb_g"])?;
        let tips = mean(["lafngrt_g", "lbfngrt_g", "lcfngrt_g", "ldfngrt_g"])?;
        let palm = crate::aim_pose::support_palm(at("lhand_g")?, knuckles, tips);
        Some(palm.distance(in_model.transform_point(grip)))
    });
    let muzzle = gun.transform_point(shape.muzzle);
    let barrel = (gun.rotation * shape.barrel).normalize_or_zero();
    let target = point * UNIT;
    let head = body.head.as_ref().and_then(|bone| bone.facing(transforms));
    let head = head.map(|(at, facing)| (root.transform_point(at), root.rotation * facing));
    let degrees = |a: Vec3, b: Vec3| a.angle_between(b).to_degrees();
    let face = body.head.as_ref().and_then(|bone| bone.face(transforms));
    let seat = in_model.transform_point(shape.seat);
    let face_from_seat_m = face.map(|(at, ..)| {
        let off = at - seat;
        [off.dot(ahead), off.z, off.dot(right)]
    });
    let jaw = body.head.as_ref().and_then(|bone| bone.jaw(transforms));
    let jaw_over_seat_m =
        jaw.map(|jaw| (jaw - seat).dot((in_model.rotation * shape.up).normalize()));
    let face = face.map(|(at, looks, top)| {
        let (at, looks, top) = (
            root.transform_point(at),
            root.rotation * looks,
            root.rotation * top,
        );
        // The roll: how far the face's top leans to the right of the
        // upright plane along its look.
        let right = looks.cross(Vec3::Z).normalize_or_zero();
        let roll = top.dot(right).clamp(-1.0, 1.0).asin().to_degrees();
        (degrees(looks, target - at), roll)
    });
    Some(crate::aim_pose::Sight {
        raise,
        muzzle,
        target,
        barrel_off_deg: degrees(barrel, target - muzzle),
        head_off_gun_deg: head.map(|(_, facing)| degrees(facing, barrel)),
        head_off_target_deg: head.map(|(at, facing)| degrees(facing, target - at)),
        push_back_m: push.back_m,
        shoulder_deg: push.shoulder_deg,
        stock_m,
        aside_m,
        support_gap_m,
        face_off_target_deg: face.map(|(off, _)| off),
        face_roll_deg: face.map(|(_, roll)| roll),
        face_from_seat_m,
        jaw_over_seat_m,
    })
}

/// A gun held up (`gun_lines`): the hand bone that carries it, its weapon
/// node, its muzzle and barrel in that node's space, the point it aims at
/// in the model's space, and its kick up (radians, `Aim::kick`).
struct GunLine {
    hand: usize,
    weapon: usize,
    muzzle: Vec3,
    barrel: Vec3,
    target: Vec3,
    kick: f32,
}

/// Each gun of `aims` held up and not in its holster (`GunLine`), on the
/// body whose rig is `rig`.
fn gun_lines(rig: &BodyRig, aims: &[Aim]) -> Vec<GunLine> {
    let model = rig.model();
    let find = |name: &str| {
        (0..model.nodes.len()).find(|&node| model.nodes[node].name.eq_ignore_ascii_case(name))
    };
    (aims.iter())
        .filter(|aim| aim.weight > 0.0 && aim.stow.is_none_or(|stow| stow.share < 1e-3))
        .filter_map(|aim| {
            let (hand, weapon) = match aim.side {
                Side::Right => (find("rhand_g")?, find("rhand")?),
                Side::Left => (find("lhand_g")?, find("lhand")?),
            };
            Some(GunLine {
                hand,
                weapon,
                muzzle: aim.muzzle,
                barrel: aim.barrel,
                target: aim.target,
                kick: aim.kick.0 * aim.weight,
            })
        })
        .collect()
}

/// Turns each gun of `lines` by its hand bone at the wrist so that its
/// barrel runs from where its muzzle now stands to the point it aims at,
/// lifted by its kick, `share` of the way (how far the guns are raised, 0
/// to 1). What turned the chest after the arms were posed (the breathing,
/// the lean of sustained fire) then moves a raised gun but does not swing
/// it off what it aims at.
fn reaim(
    lines: &[GunLine],
    (nodes, rest): (&[Entity], &Rest),
    share: f32,
    transforms: &mut impl Transforms,
) {
    if share <= 0.0 {
        return;
    }
    for line in lines {
        let (Some(placed), Some(wrist)) = (
            placed_in_model(transforms, (nodes, rest), line.weapon),
            placed_in_model(transforms, (nodes, rest), line.hand),
        ) else {
            continue;
        };
        let (muzzle, wrist) = (placed.transform_point(line.muzzle), wrist.translation);
        let Some(now) = (placed.rotation * line.barrel).try_normalize() else {
            continue;
        };
        // The turn at the wrist carries the muzzle too, which changes the
        // way from the muzzle to the point: found again from where the turn
        // leaves the muzzle, a few times over.
        let mut turn = Quat::IDENTITY;
        for _ in 0..REAIM_STEPS {
            let moved = wrist + turn * (muzzle - wrist);
            let Some(way) = (line.target - moved).try_normalize() else {
                break;
            };
            turn = Quat::from_rotation_arc(now, asset_ja::rig::kicked(way, line.kick));
        }
        let turn = Quat::IDENTITY.slerp(turn, share.min(1.0));
        turn_in_model(transforms, (nodes, rest), line.hand, turn);
    }
}

/// Puts the support hand of `aim`'s gun back on it (`Retargeter::regrip`)
/// on the body whose rig is `rig`, its nodes posed as they stand.
fn regrip<T: Transforms>(rig: &BodyRig, aim: &Aim, nodes: &[Entity], transforms: &mut T) {
    if aim.support.is_none() || aim.stow.is_some_and(|stow| stow.share > 0.0) {
        return;
    }
    let rest = &rig.rest;
    let placed = |transforms: &T, node: usize| placed_in_model(transforms, (nodes, rest), node);
    let globals: Option<Vec<(Vec3, Quat)>> = (0..rest.len())
        .map(|node| placed(transforms, node).map(|t| (t.translation, t.rotation)))
        .collect();
    let Some(globals) = globals else {
        return;
    };
    for (node, global) in rig.retargeter.regrip(&globals, aim) {
        let parent = (rest.get(node).and_then(|r| r.0))
            .and_then(|parent| placed(transforms, parent))
            .map_or(Quat::IDENTITY, |t| t.rotation);
        if let Some(&entity) = nodes.get(node) {
            edit(transforms, entity, |t| {
                t.rotation = (parent.inverse() * global).normalize();
            });
        }
    }
}

/// How many times `reaim` finds its turn again from where the last one left
/// the muzzle. The muzzle stands under a metre from the wrist and a target
/// at least a metre out (`aim_pose::NEAREST_SIGHT`), so each time takes most
/// of what is left off.
const REAIM_STEPS: usize = 4;

/// Node `node`'s place in its model's space as `transforms` pose it now: its
/// own transform under each of its parents' (`rest`'s, `nodes` their
/// entities).
fn placed_in_model(
    transforms: &impl Transforms,
    (nodes, rest): (&[Entity], &Rest),
    node: usize,
) -> Option<Transform> {
    let mut chain = vec![node];
    while let Some(parent) = rest.get(*chain.last()?)?.0 {
        chain.push(parent);
    }
    (chain.iter().rev()).try_fold(Transform::IDENTITY, |above, &at| {
        Some(above.mul_transform(*transforms.read(*nodes.get(at)?)?))
    })
}

/// The share of the clips laid over a JA pose from which they cover it
/// whole.
const WHOLE: f32 = 0.999;

/// How fast a head turns toward whom its body looks at in a conversation,
/// and back (radians a second): KOTOR's look-at speed is 10, in a unit
/// not read [verify].
const HEAD_TURN_RAD_S: f32 = 10.0;

/// The bone a creature turns its head by when it looks at someone, where
/// its `appearance.2da` row names none (`headbone`, blank in every row of
/// both games): the neck's top, so the neck stays on the torso.
const HEAD_TURN_BONE: &str = "hturn_g";

/// The node a body's head turns by (S7) and the nodes above it, from the
/// body model's root down (across the head model hung on it), with the
/// turn each takes in the model's space at rest: the head's (`rest`) and
/// the chest's it hangs from (`chest`, the last of `above` from the body).
struct HeadBone {
    node: Entity,
    above: Vec<Entity>,
    rest: Quat,
    chest: (usize, Quat),
    /// The upper spine the head rides on, straightened with it (`level`).
    spine: Option<Spine>,
    /// The face's own node under the head's (`head_g` under `hturn_g`),
    /// which a head model's clips turn on their own, with its turn in the
    /// model's space at rest. None where the head's node is the face's.
    face: Option<(Entity, Quat)>,
    /// The head model's neck and face (`neck_g`, `head_g`), which its
    /// standing clip turns as the head looks about, each with its own turn
    /// at rest: held near rest while a gun is raised (`AIM_STILL`).
    still: Vec<(Entity, Quat)>,
    /// The bottom of the face's jaw in the face's own node's space
    /// (`aim_pose::jaw_of`), for the cheek on a rifle's stock.
    jaw: Option<Vec3>,
}

/// How much of the head model's own looking about (its standing clip's
/// turns of its neck and face, `HeadBone::still`) a raised gun takes away,
/// at full raise. The rest is left so the head still looks alive: the
/// standing clip swings Carth's face up to 11 degrees, so under one degree
/// is left.
const AIM_STILL: f32 = 0.92;

/// A body's spine (`SPINE`) where the head's way passes it: its joints low
/// to high, each a child of the one before; the nodes above the lowest
/// from the model's root down, with their turn in the model's space at
/// rest; and the top joint's children off the head's way (the collars),
/// each with its own children (the upper arms).
struct Spine {
    joints: Vec<Joint>,
    base: Vec<Entity>,
    base_rest: Quat,
    collars: Vec<(Entity, Vec<Entity>)>,
}

/// A spine joint: its node and its turn in its parent's space at rest.
struct Joint {
    node: Entity,
    rest: Quat,
}

/// The joints of KOTOR's spine, low to high: the lower back, bent at the
/// waist, and the upper back the collars and the neck hang from.
const SPINE: [&str; 2] = [hooks::TORSO, hooks::UPPER_TORSO];

/// How much of its bend each spine joint of a body held level
/// (`HeadBone::level`) gives back: the neck turns that much less to hold
/// the head level, and the back looks less hunched; a crouch keeps a slight
/// hunch with it too (tuning, view only).
const SPINE_SHARE: f32 = 0.4;

/// `body`'s spine (`Spine`), where the head's way up from body node `end`
/// (the node the head model hangs at, or the head itself) passes it: its
/// joints there that hang one from the next, else its top one alone.
fn spine_of((ir, nodes): (&ModelIr, &[Entity]), end: usize) -> Option<Spine> {
    let way: Vec<usize> =
        std::iter::successors(Some(end), |&at| ir.nodes.get(at).and_then(|n| n.parent)).collect();
    let mut found: Vec<usize> = (SPINE.iter())
        .filter_map(|name| (way.iter()).find(|&&at| ir.nodes[at].name.eq_ignore_ascii_case(name)))
        .copied()
        .collect();
    if found
        .windows(2)
        .any(|pair| ir.nodes[pair[1]].parent != Some(pair[0]))
    {
        found.drain(..found.len() - 1);
    }
    let (&low, &top) = (found.first()?, found.last()?);
    let base = ancestors((ir, nodes), low);
    let joints = (found.iter())
        .map(|&at| at_rest_of((ir, nodes), at).map(|(node, rest)| Joint { node, rest }))
        .collect::<Option<_>>()?;
    let children =
        |of: usize| (ir.nodes.iter().enumerate()).filter(move |(_, n)| n.parent == Some(of));
    let collars = children(top)
        .filter(|(at, _)| !way.contains(at))
        .filter_map(|(at, _)| {
            let arms = children(at)
                .filter_map(|(arm, _)| nodes.get(arm).copied())
                .collect();
            Some((*nodes.get(at)?, arms))
        })
        .collect();
    Some(Spine {
        joints,
        base_rest: (base.iter()).fold(Quat::IDENTITY, |turn, (_, rotation)| turn * *rotation),
        base: base.into_iter().map(|(entity, _)| entity).collect(),
        collars,
    })
}

impl Spine {
    /// Each joint, low to high, bent back toward its parent's up by `share`
    /// of its bend from it (a turn across, its twist kept), so the back
    /// straightens all along and no joint bends backward. `keep` (a weapon
    /// in hand): the collars set again where they stood, so the arms, the
    /// hands and what they hold stay where the pose put them (the drawn
    /// blade on the sim's). Else the shoulders ride on the chest, and the
    /// upper arms keep the way the pose swings them.
    fn straighten<T: Transforms>(&self, share: f32, keep: bool, transforms: &mut T) -> Vec<Set> {
        let affine = |transforms: &T, entity: Entity| {
            transforms
                .read(entity)
                .map_or(Default::default(), |t| t.compute_affine())
        };
        let chain = |transforms: &T| {
            (self.joints.iter()).fold(bevy::math::Affine3A::IDENTITY, |chain, joint| {
                chain * affine(transforms, joint.node)
            })
        };
        let turn = |affine: bevy::math::Affine3A| affine.to_scale_rotation_translation().1;
        // Before: the collars where they stand, or the arms (with their
        // collar) as they turn, in the spine's parent's space.
        let before = chain(transforms);
        let stood: Vec<(Entity, Option<Entity>, bevy::math::Affine3A)> = (self.collars.iter())
            .flat_map(|(collar, arms)| {
                let on = before * affine(transforms, *collar);
                if keep {
                    vec![(*collar, None, on)]
                } else {
                    let arm = |&arm: &Entity| (arm, Some(*collar), on * affine(transforms, arm));
                    arms.iter().map(arm).collect()
                }
            })
            .collect();
        let rotation = |entity: Entity| {
            transforms
                .read(entity)
                .map_or(Quat::IDENTITY, |t| t.rotation)
        };
        let mut world = (self.base.iter()).fold(Quat::IDENTITY, |turn, &e| turn * rotation(e));
        let mut rest = self.base_rest;
        for joint in &self.joints {
            let Some(own) = transforms.read(joint.node).map(|t| t.rotation) else {
                return Vec::new();
            };
            let parent_up = (world * rest.inverse()) * Vec3::Z;
            rest *= joint.rest;
            let up = (world * own * rest.inverse()) * Vec3::Z;
            let axis = up.cross(parent_up).normalize_or_zero();
            let turned = if axis == Vec3::ZERO {
                own
            } else {
                let back = Quat::from_axis_angle(axis, up.angle_between(parent_up) * share);
                (world.inverse() * back * world * own).normalize()
            };
            edit(transforms, joint.node, |t| t.rotation = turned);
            world *= turned;
        }
        let after = chain(transforms);
        let mut set = Vec::new();
        for (entity, collar, stood) in stood {
            let Some(&from) = transforms.read(entity) else {
                continue;
            };
            let written = match collar {
                None => Transform::from_matrix(Mat4::from(after.inverse() * stood)),
                Some(collar) => {
                    let on = turn(after * affine(transforms, collar));
                    from.with_rotation((on.inverse() * turn(stood)).normalize())
                }
            };
            edit(transforms, entity, |t| *t = written);
            set.push(Set {
                node: entity,
                from,
                written,
            });
        }
        set
    }
}

/// A node the spine's straightening set (`Spine::straighten`): as it stood
/// before, and as written. A node the body's pose leaves as it is (one its
/// standing clip does not key, `held_nodes`, such as a woman's lower neck)
/// is put back as it stood (`Head::turn`) before it is set again, or each
/// frame's setting would build on the last: the frames, not the time, ran
/// it up (issue 238, a shoulder drawn metres out at a high frame rate).
struct Set {
    node: Entity,
    from: Transform,
    written: Transform,
}

impl Set {
    /// Puts back, part by part, what still stands as it was written: a
    /// pose may write a node's turn afresh and leave its place (the
    /// retargeted pose turns the nodes that carry others, never moves them),
    /// so its place goes back though its turn is the pose's.
    fn undo(&self, t: &mut Transform) {
        if t.translation == self.written.translation {
            t.translation = self.from.translation;
        }
        if t.rotation == self.written.rotation {
            t.rotation = self.from.rotation;
        }
        if t.scale == self.written.scale {
            t.scale = self.from.scale;
        }
    }
}

/// A head model hung on its body: its model and nodes, the body node it
/// hangs at (`headhook`), and its root's entity and turn there.
struct Hung<'a> {
    ir: &'a ModelIr,
    nodes: &'a [Entity],
    hook: usize,
    root: (Entity, Quat),
}

/// `body`'s head bone (S7): `HEAD_TURN_BONE` in the head model hung on it
/// (`hung`, at body node `hook`), else in its own model; without one, its
/// head (`head_g`), else a creature model's `head` (HK-47's, T3-M4's
/// `Head`).
fn head_bone(body: (&ModelIr, &[Entity]), hung: Option<Hung>) -> Option<HeadBone> {
    let find = |ir: &ModelIr, name: &str| {
        (ir.nodes.iter()).position(|node| node.name.eq_ignore_ascii_case(name))
    };
    let turned = |chain: &[(Entity, Quat)]| {
        (chain.iter()).fold(Quat::IDENTITY, |above, (_, rotation)| above * *rotation)
    };
    [HEAD_TURN_BONE, hooks::HEAD, "head"]
        .iter()
        .find_map(|name| {
            // The chest: the hook, else the head's own parent; and the
            // body's node the head's way leaves it at.
            // The face: a child of the head's node named `head_g`.
            let face_at = |ir: &ModelIr, index: usize| {
                (ir.nodes.iter()).position(|n| {
                    n.parent == Some(index) && n.name.eq_ignore_ascii_case(hooks::HEAD)
                })
            };
            let face_of = |(ir, nodes): (&ModelIr, &[Entity]), index: usize| {
                face_at(ir, index).and_then(|at| at_rest_of((ir, nodes), at))
            };
            // The bottom of the face's jaw, for the cheek on a stock.
            let jaw_of = |ir: &ModelIr, index: usize| {
                face_at(ir, index).and_then(|at| crate::aim_pose::jaw_of(ir, at))
            };
            let (mut chain, node, chest, end, face, still, jaw) = match &hung {
                Some(hung) if let Some(index) = find(hung.ir, name) => {
                    let mut chain = ancestors(body, hung.hook);
                    let chest = chain.len();
                    chain.push(at_rest_of(body, hung.hook)?);
                    chain.push(hung.root);
                    chain.extend(ancestors((hung.ir, hung.nodes), index));
                    let node = at_rest_of((hung.ir, hung.nodes), index)?;
                    let face = face_of((hung.ir, hung.nodes), index);
                    let still = (chain.last().copied().into_iter()).chain(face).collect();
                    let jaw = jaw_of(hung.ir, index);
                    (chain, node, chest, hung.hook, face, still, jaw)
                }
                _ => {
                    let index = find(body.0, name)?;
                    let chain = ancestors(body, index);
                    let chest = chain.len().checked_sub(1)?;
                    let face = face_of(body, index);
                    (
                        chain,
                        at_rest_of(body, index)?,
                        chest,
                        index,
                        face,
                        Vec::new(),
                        jaw_of(body.0, index),
                    )
                }
            };
            let chest_rest = turned(&chain[..=chest]);
            let rest = turned(&chain) * node.1;
            let face = face.map(|(entity, own)| (entity, rest * own));
            let above = chain.drain(..).map(|(entity, _)| entity).collect();
            Some(HeadBone {
                node: node.0,
                above,
                rest,
                chest: (chest, chest_rest),
                spine: spine_of(body, end),
                face,
                still,
                jaw,
            })
        })
}

/// Node `node` of `ir` (its entity among `nodes`) and its turn at rest.
fn at_rest_of((ir, nodes): (&ModelIr, &[Entity]), node: usize) -> Option<(Entity, Quat)> {
    let [x, y, z, w] = ir.nodes.get(node)?.rotation;
    Some((*nodes.get(node)?, Quat::from_xyzw(x, y, z, w).normalize()))
}

/// The entities of `node`'s ancestors in `ir`, from its root down, each
/// with its turn at rest.
fn ancestors((ir, nodes): (&ModelIr, &[Entity]), node: usize) -> Vec<(Entity, Quat)> {
    let mut above = Vec::new();
    let mut parent = ir.nodes.get(node).and_then(|n| n.parent);
    while let Some(index) = parent {
        above.extend(at_rest_of((ir, nodes), index));
        parent = ir.nodes[index].parent;
    }
    above.reverse();
    above
}

/// How far (degrees) a body's chest may lean from upright and its head
/// still be held level: wholly up to the first, not at all from the
/// second (a roll, a flip, a knockdown: the head goes with the body).
const LEVEL_LEAN_DEG: (f32, f32) = (55.0, 85.0);

/// The part of `turn` about the model's up (its twist): `turn` less its
/// tilt. None where `turn` tips the up over (no heading to keep).
fn twist_about_up(turn: Quat) -> Option<Quat> {
    let twist = Quat::from_xyzw(0.0, 0.0, turn.z, turn.w);
    (twist.length_squared() > 1e-6).then(|| twist.normalize())
}

/// The heading of JA's head in `pose` (radians about the model's up from
/// its base pose): its `cranium`'s turn less its tilt, so the clip's twist
/// of the body, a spin, and the cgame's look toward the view. None for a
/// skeleton without one, or a head tipped over.
fn ja_head_heading(g2: &G2, pose: &[Local]) -> Option<f32> {
    let cranium = g2.bone("cranium")?;
    let twist = twist_about_up(g2.bone_final(pose, cranium).0)?;
    Some(2.0 * twist.z.atan2(twist.w))
}

impl HeadBone {
    /// Where the head's node stands in the model's space (metres) and which
    /// way the head faces there, its nodes posed as they stand. The body
    /// faces +Y at rest, so the face points where the head's turn from its
    /// rest (`rest`) takes +Y.
    fn facing(&self, transforms: &impl Transforms) -> Option<(Vec3, Vec3)> {
        let placed = (self.above.iter().chain([&self.node]))
            .map(|&e| transforms.read(e).copied())
            .try_fold(Transform::IDENTITY, |above, own| {
                Some(above.mul_transform(own?))
            })?;
        Some((
            placed.translation,
            placed.rotation * self.rest.inverse() * Vec3::Y,
        ))
    }

    /// Where the bottom of the face's jaw (`jaw`) stands in the model's
    /// space, its nodes posed as they stand. None for a head with no face
    /// node under its own.
    fn jaw(&self, transforms: &impl Transforms) -> Option<Vec3> {
        let ((face, _), jaw) = (self.face?, self.jaw?);
        let placed = (self.above.iter().chain([&self.node, &face]))
            .map(|&e| transforms.read(e).copied())
            .try_fold(Transform::IDENTITY, |above, own| {
                Some(above.mul_transform(own?))
            })?;
        Some(placed.transform_point(jaw))
    }

    /// Where the face's node stands in the model's space (metres), which way
    /// the face looks and which way its top points, its nodes posed as they
    /// stand: the face's own node (`face`) where it has one, else the
    /// head's.
    fn face(&self, transforms: &impl Transforms) -> Option<(Vec3, Vec3, Vec3)> {
        let (last, rest) = self.face.map_or((None, self.rest), |(e, r)| (Some(e), r));
        let placed = (self.above.iter().chain([&self.node]).chain(last.as_ref()))
            .map(|&e| transforms.read(e).copied())
            .try_fold(Transform::IDENTITY, |above, own| {
                Some(above.mul_transform(own?))
            })?;
        let turn = placed.rotation * rest.inverse();
        Some((placed.translation, turn * Vec3::Y, turn * Vec3::Z))
    }

    /// The head held level, heading where JA's own head heads
    /// (`hold.heading`, else where the pose turned it): its turn in the
    /// model's space set to its rest turned about the model's up by that
    /// much, so it stands up whatever the clip does with the chest and
    /// turns with a spin or a swing as JA's does. By `share` (the JA pose's
    /// share of the body left to hold), less as the chest leans far from
    /// upright unless the legs crouch (`crouch`, 0 to 1: how far into a
    /// crouch's hold).
    fn level<T: Transforms>(
        &self,
        hold: Hold,
        (share, crouch): (f32, f32),
        transforms: &mut T,
    ) -> Vec<Set> {
        let read = |transforms: &T| -> Vec<Quat> {
            (self.above.iter())
                .map(|&e| transforms.read(e).map_or(Quat::IDENTITY, |t| t.rotation))
                .collect()
        };
        let mut rotations = read(transforms);
        let product = |rotations: &[Quat]| {
            (rotations.iter()).fold(Quat::IDENTITY, |above, rotation| above * *rotation)
        };
        let (chest, chest_rest) = self.chest;
        let Some(chest_now) = rotations.get(..=chest).map(product) else {
            return Vec::new();
        };
        let lean = ((chest_now * chest_rest.inverse()) * Vec3::Z)
            .dot(Vec3::Z)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        let (whole, none) = LEVEL_LEAN_DEG;
        // A crouch bows the chest as far as a roll does, but its legs stand:
        // its head is held whatever the lean.
        let upright = ((none - lean) / (none - whole)).clamp(0.0, 1.0).max(crouch);
        let weight = share.clamp(0.0, 1.0) * upright;
        if weight <= 0.0 {
            return Vec::new();
        }
        // The spine straightened first, so the neck turns the less; a
        // weapon in hand keeps the arms where the pose holds it.
        let set = match &self.spine {
            Some(spine) => {
                let set = spine.straighten(SPINE_SHARE * weight, hold.armed, transforms);
                rotations = read(transforms);
                set
            }
            None => Vec::new(),
        };
        let above = product(&rotations);
        let Some(own) = transforms.read(self.node).map(|t| t.rotation) else {
            return set;
        };
        let heading = match hold.heading {
            Some(heading) => Quat::from_rotation_z(heading),
            None => match twist_about_up(above * own * self.rest.inverse()) {
                Some(heading) => heading,
                None => return set,
            },
        };
        let local = (above.inverse() * heading * self.rest).normalize();
        edit(transforms, self.node, |t| {
            t.rotation = t.rotation.slerp(local, weight).normalize();
        });
        set
    }
}

/// A head turned toward whom its body looks at in a conversation's line
/// (S7): how far from the body (degrees, to the left), as of when (sim
/// ms), and its node's turn as last written and the turn it was written
/// over.
#[derive(Default)]
struct Head {
    yaw: f32,
    ms: Option<f64>,
    turned: Option<(Quat, Quat)>,
    /// The nodes the spine's straightening set last (`Spine::straighten`).
    set: Vec<Set>,
    /// How much of the level hold (`HeadBone::level`) has let go since its
    /// body died (0 alive, 1 dead `LEVEL_FADE_S` and more).
    let_go: f32,
    /// How far its body is into a crouch (`Hold::crouched`): 0 out of one,
    /// 1 crouched `CROUCH_EASE_S` and more.
    crouch: f32,
    /// The neck as the cheek weld last wrote it, and as it stood before
    /// (`aim_pose::weld_turn`).
    welded: Option<(Transform, Transform)>,
    /// How far (metres, the model's space) the head has come down onto a
    /// rifle's stock past the cheek weld's turn (`aim_pose::cheek_drop`),
    /// eased over `CHEEK_DROP_EASE_S`.
    dropped: Vec3,
}

/// How long (seconds) a head takes to come about two thirds of the way
/// down onto a rifle's stock past the cheek weld (`Head::dropped`), and
/// back up. A shot's kick turns the rifle for a moment, and the head does
/// not jerk with it.
const CHEEK_DROP_EASE_S: f32 = 0.08;

/// How far (0 to 1) a rifle must be raised to the shoulder before the head
/// comes down onto its stock past the cheek weld (`Head::dropped`). On its
/// way up the stock still stands low, under the jaw.
const SEATED: f32 = 0.999;

/// What a head is held level by (`HeadBone::level`): JA's head's heading
/// (`ja_head_heading`), the JA pose's share of the body, whether the body
/// lives, whether its legs crouch, and whether it has a weapon out.
#[derive(Clone, Copy)]
struct Hold {
    heading: Option<f32>,
    share: f32,
    alive: bool,
    crouched: bool,
    armed: bool,
}

/// How long (seconds) a body that dies takes to let its head go from level
/// to its death's clip, and one revived to take it back: at once, the head
/// jumped as far as it was held.
const LEVEL_FADE_S: f32 = 0.3;

/// How long (seconds) a body's head takes to come into a crouch's hold, and
/// out of it (a roll from a crouch lets the head go with the body).
const CROUCH_EASE_S: f32 = 0.2;

/// Whether a body's legs crouch, standing or walking (not a roll's duck).
fn crouches(state: &MotionState) -> bool {
    matches!(
        state.legs.anim,
        Anim::BOTH_CROUCH1IDLE | Anim::BOTH_CROUCH1WALK | Anim::BOTH_CROUCH1WALKBACK
    )
}

impl Head {
    /// At `ms`, eases toward `target` (degrees from the body; none: ahead)
    /// at `HEAD_TURN_RAD_S` and turns `bone` so far about the model's up
    /// after the body's pose (`turn_under`), the head first held level
    /// (`HeadBone::level`) where `level` says, letting go over
    /// `LEVEL_FADE_S` once its body is dead. A body aiming a raised gun then
    /// looks down it at the point its gun aims at (`sight`: that point in
    /// the model's space, and how far the gun is raised, 0 to 1): its face
    /// turns that share of the way toward the point, so at full raise it
    /// looks straight at it and does not drift with the pose. Last it leans
    /// by `tilt`: a turn (radians) about an axis in the model's space. A
    /// node no pose moved since it was turned turns from where it stood
    /// before. While it looks down a gun, the lean turns about the way the
    /// face looks. Before it looks, a rifle's cheek weld (`sight`'s last
    /// part, `aim_pose::cheek_weld_deg`) turns the neck (the head node's
    /// parent), that share of the way, so the head comes down onto the
    /// stock, and moves it back with the stock as far as the last shot
    /// pushes the gun (metres, the weld's second part). A head whose jaw
    /// then stands too high over the rifle's seat in the shoulder, along the
    /// way the rifle's top faces (both in the model's space, the weld's
    /// last part), comes down that much further (`aim_pose::cheek_drop`).
    /// The neck, not the node the head model hangs from (`headhook`): a KOTOR head's mesh
    /// starts at the collar with a ring its lower neck alone moves, so a
    /// turn where the model hangs would pull that ring off the body's
    /// collar and open a seam, where the neck bends the mesh above it. The
    /// neck turns about the head model's hanging point, so the head comes
    /// down as far as before. A raised gun first holds the head model's own
    /// looking about (its standing clip's turns of its neck and face) near
    /// rest, that share of the way times `AIM_STILL`.
    #[allow(clippy::type_complexity)]
    fn turn<T: Transforms>(
        &mut self,
        (target, ms): (Option<f32>, f64),
        (level, tilt, sight): (
            Option<Hold>,
            Option<(Vec3, f32)>,
            Option<(Vec3, f32, Option<([f32; 2], f32, Option<(Vec3, Vec3)>)>)>,
        ),
        bone: &HeadBone,
        transforms: &mut T,
    ) {
        let seconds = self
            .ms
            .map_or(0.0, |was| ((ms - was) / 1000.0).clamp(0.0, 0.25)) as f32;
        self.ms = Some(ms);
        let most = HEAD_TURN_RAD_S.to_degrees() * seconds;
        self.yaw += (target.unwrap_or(0.0) - self.yaw).clamp(-most, most);
        let ease = |at: &mut f32, to: bool, over: f32| {
            let step = seconds / over;
            *at += (f32::from(u8::from(to)) - *at).clamp(-step, step);
        };
        let level = level.filter(|_| !audit_off("head")).and_then(|hold| {
            ease(&mut self.let_go, !hold.alive, LEVEL_FADE_S);
            ease(&mut self.crouch, hold.crouched, CROUCH_EASE_S);
            let share = hold.share * (1.0 - self.let_go);
            (share > 0.0).then_some((hold, (share, self.crouch)))
        });
        // What the straightening set that no pose wrote since stands as it
        // stood before it, so it is straightened from there.
        for set in std::mem::take(&mut self.set) {
            edit(transforms, set.node, |t| set.undo(t));
        }
        // The weld from the frame before, where no pose wrote the neck since.
        let neck = (bone.above.len().checked_sub(1)).map(|at| (at, bone.above[at]));
        if let (Some((written, from)), Some((_, neck))) = (self.welded.take(), neck)
            && transforms.read(neck).is_some_and(|t| *t == written)
        {
            edit(transforms, neck, |t| *t = from);
        }
        let tilt = tilt.filter(|&(_, angle)| angle.abs() > 1e-4);
        let sight = sight.filter(|&(_, share, _)| share > 1e-4);
        // A raised gun holds the head model's own looking about near rest,
        // as far as the gun is raised.
        if let Some((_, share, _)) = sight {
            for &(node, rest) in &bone.still {
                edit(transforms, node, |t| {
                    t.rotation = t.rotation.slerp(rest, share * AIM_STILL).normalize();
                });
            }
        }
        if self.yaw.abs() < 0.01
            && self.turned.is_none()
            && level.is_none()
            && tilt.is_none()
            && sight.is_none()
        {
            return;
        }
        let entity = bone.node;
        let Some(now) = transforms.read(entity).map(|t| t.rotation) else {
            return;
        };
        let from = match self.turned {
            Some((written, from)) if written == now => from,
            _ => now,
        };
        edit(transforms, entity, |t| t.rotation = from);
        if let Some((hold, shares)) = level {
            self.set = bone.level(hold, shares, transforms);
        }
        if self.yaw.abs() >= 0.01 {
            let turn = Quat::from_rotation_z(self.yaw.to_radians());
            turn_under(transforms, &bone.above, entity, turn);
        }
        if let Some((point, share, Some((weld, back, seat)))) = sight
            && let Some((index, neck)) = neck
            && let Some((at, _)) = bone.facing(transforms)
            && let Some(from) = transforms.read(neck).copied()
        {
            let above = &bone.above[..index];
            let turn = crate::aim_pose::weld_turn(point - at, weld, share);
            turn_under(transforms, above, neck, turn);
            // The neck turns about where the head model hangs (the base of
            // the neck), not about its own joint behind it, so the head
            // comes down onto the stock as far as a turn there takes it.
            let placed = |transforms: &T, chain: &[Entity]| {
                (chain.iter()).try_fold(Transform::IDENTITY, |above, &e| {
                    Some(above.mul_transform(*transforms.read(e)?))
                })
            };
            let base = placed(transforms, &bone.above[..=bone.chest.0]);
            let joint = placed(transforms, &bone.above[..=index]);
            if let (Some(parent), Some(joint), Some(base)) =
                (placed(transforms, above), joint, base)
            {
                let level = ((point - at) * Vec3::new(1.0, 1.0, 0.0)).normalize_or_zero();
                let from_base = joint.translation - base.translation;
                let pivot = turn * from_base - from_base;
                let moved = parent.rotation.inverse() * (pivot - level * back) / parent.scale;
                edit(transforms, neck, |t| t.translation += moved);
                // A head whose jaw still stands too high over the stock's
                // seat comes down onto it (`aim_pose::cheek_drop`).
                let wanted = match (seat, bone.jaw(transforms)) {
                    (Some((seat, up)), Some(jaw)) => {
                        up * -crate::aim_pose::cheek_drop((jaw - seat).dot(up))
                    }
                    _ => Vec3::ZERO,
                };
                let eased = 1.0 - (-seconds / CHEEK_DROP_EASE_S).exp();
                self.dropped += (wanted - self.dropped) * eased;
                if self.dropped != Vec3::ZERO {
                    let lowered = parent.rotation.inverse() * (self.dropped * share) / parent.scale;
                    edit(transforms, neck, |t| t.translation += lowered);
                }
            }
            self.welded = transforms.read(neck).map(|t| (*t, from));
        }
        let mut tilt = tilt;
        if let Some((point, share, _)) = sight
            && let Some((at, facing)) = bone.facing(transforms)
            && let Some(way) = (point - at).try_normalize()
        {
            let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(facing, way), share);
            turn_under(transforms, &bone.above, entity, turn);
            // The lean then turns about the way the face looks, so it does
            // not swing the face off the point.
            let looks = (turn * facing).normalize_or_zero();
            tilt = tilt.map(|(axis, angle)| (axis.lerp(looks, share).normalize_or(axis), angle));
        }
        if let Some((axis, angle)) = tilt {
            let turn = Quat::from_axis_angle(axis, angle);
            turn_under(transforms, &bone.above, entity, turn);
        }
        self.turned = transforms
            .read(entity)
            .map(|t| (t.rotation, from))
            .filter(|(written, from)| written != from);
    }
}

/// A head held level over a leaning spine with a weapon out, on a skeleton
/// made here (`kjk::probes::straightened`, issue 238): the spine's two
/// joints, the head on its top, and beside the head a node no standing
/// clip keys (`held_nodes`, a woman's lower neck), which each frame's pose
/// turns afresh but never moves. Posed alike 2000 times at 2.5 ms a frame
/// (400 fps), the held node is set alike every frame, no further out than the
/// first frame set it, where each setting once built on the last until it
/// stood a metre out; and
/// with the weapon put away it stands where it stood before.
#[cfg(feature = "probes")]
pub fn straighten_probes() -> crate::probe_kit::Checks {
    use crate::probe_kit::check;
    use asset_odyssey::model::NodeIr;
    use bevy::ecs::system::SystemState;

    let mut ir = crate::course::model_of("body", Vec::new());
    let root = ir.nodes[0].clone();
    for (name, parent, z) in [
        (hooks::TORSO, 0, 1.0),
        (hooks::UPPER_TORSO, 1, 0.3),
        (hooks::HEAD, 2, 0.3),
        ("necklwr_g", 2, 0.28),
        ("neck_g", 4, 0.03),
    ] {
        ir.nodes.push(NodeIr {
            name: name.into(),
            parent: Some(parent),
            translation: [0.0, 0.0, z],
            ..root.clone()
        });
    }
    let (held, neck) = (4, 5);
    let mut world = World::new();
    let nodes: Vec<Entity> = (ir.nodes.iter())
        .map(|node| {
            let at = Transform::from_translation(Vec3::from_array(node.translation));
            world.spawn(at).id()
        })
        .collect();
    let Some(bone) = head_bone((&ir, &nodes), None) else {
        return vec![("head bone", Err("none found".into()))];
    };
    let rest = Vec3::from_array(ir.nodes[held].translation);
    let mut state: SystemState<Query<&mut Transform>> = SystemState::new(&mut world);
    let mut head = Head::default();
    let lean = [
        Quat::from_rotation_x(15f32.to_radians()),
        Quat::from_rotation_x(10f32.to_radians()),
    ];
    let mut frame = |n: u32, armed: bool, world: &mut World| -> Transform {
        let Ok(mut transforms) = state.get_mut(world) else {
            return Transform::IDENTITY;
        };
        // The pose: the spine leant, the held node and the neck turned, the
        // held node's place left as it is.
        for (node, turn) in [(1, lean[0]), (2, lean[1]), (held, Quat::IDENTITY)] {
            edit(&mut transforms, nodes[node], |t| t.rotation = turn);
        }
        edit(&mut transforms, nodes[neck], |t| {
            t.rotation = Quat::IDENTITY
        });
        let hold = Hold {
            heading: None,
            share: 1.0,
            alive: true,
            crouched: false,
            armed,
        };
        head.turn(
            (None, f64::from(n) * 2.5),
            (Some(hold), None, None),
            &bone,
            &mut transforms,
        );
        transforms
            .get(nodes[held])
            .copied()
            .unwrap_or(Transform::IDENTITY)
    };
    let first = frame(0, true, &mut world);
    let last = (1..2000).fold(first, |_, n| frame(n, true, &mut world));
    let put_away = frame(2000, false, &mut world);
    let out = |t: Transform| t.translation.distance(rest);
    vec![
        (
            "a held node set alike every frame",
            check(
                first.translation.abs_diff_eq(last.translation, 1e-5)
                    && first.rotation.abs_diff_eq(last.rotation, 1e-5),
                format!("frame 1 {first:?}, frame 2000 {last:?}"),
            ),
        ),
        (
            "a held node stays on the chest",
            check(
                out(first) > 0.0 && out(last) <= out(first) + 1e-5,
                format!(
                    "{} m from its rest after 2000 frames, {} m after the first",
                    out(last),
                    out(first)
                ),
            ),
        ),
        (
            "a held node back as it stood once the weapon is away",
            check(
                put_away.translation.distance(rest) < 1e-5,
                format!("at {:?} (rest {rest:?})", put_away.translation),
            ),
        ),
    ]
}

/// How far (degrees, to the left, within `arc` of it) a body standing at
/// `soles` (game space, metres) facing `facing` (degrees) turns its head to
/// look at the one standing at `other`.
fn look_toward(soles: Vec3, facing: f32, (other, arc): (Vec3, f32)) -> f32 {
    let to = other - soles;
    let aim = to.y.atan2(to.x).to_degrees();
    angles::wrap_180(aim - facing).clamp(-arc, arc)
}

/// Where `node` (of `nodes`, parented as `rest` has them) is drawn (world,
/// metres): its transform under its ancestors' and the body's `root`'s, as
/// they stand now.
fn drawn_at(
    transforms: &Query<&mut Transform>,
    root: Entity,
    (nodes, rest): (&[Entity], &Rest),
    node: usize,
) -> Option<Vec3> {
    let mut chain = vec![node];
    let mut parent = rest.get(node).and_then(|r| r.0);
    while let Some(index) = parent {
        chain.push(index);
        parent = rest.get(index).and_then(|r| r.0);
    }
    let mut world = transforms.get(root).ok()?.to_matrix();
    for &index in chain.iter().rev() {
        world *= transforms.get(*nodes.get(index)?).ok()?.to_matrix();
    }
    Some(world.w_axis.truncate())
}

/// Turns `node` by `turn` in the model's space, its ancestors (`rest`'s
/// parents, `nodes` their entities) posed as they stand.
fn turn_in_model(
    transforms: &mut impl Transforms,
    (nodes, rest): (&[Entity], &Rest),
    node: usize,
    turn: Quat,
) {
    let mut above = Vec::new();
    let mut parent = rest.get(node).and_then(|r| r.0);
    while let Some(index) = parent {
        above.extend(nodes.get(index));
        parent = rest.get(index).and_then(|r| r.0);
    }
    above.reverse();
    if let Some(&entity) = nodes.get(node) {
        turn_under(transforms, &above, entity, turn);
    }
}

/// The upper torso's node. A bolt that hits the trunk lower down turns it
/// too, by `bolt_hits::TRUNK_SHARE` of the flinch.
const UPPER_TORSO: &str = "torsoupr_g";

/// Turns the nodes the bolt hits `flinches` (where each hit, its way in game
/// space, and when, view ms) flinch at `ms`. Each hit turns its location's
/// node (`spots`, from `Body::flinch_nodes`) so that the node's top moves
/// along the bolt's way. A hit on the trunk turns the upper torso (`upper`)
/// too, when that is another node. The turns of one node add up, to at most
/// `bolt_hits::FLINCH_MOST_DEG`. `to_model` turns game space into the
/// model's.
fn bolt_flinch(
    (nodes, rest): (&[Entity], &Rest),
    (spots, upper): ([Option<usize>; LOCATIONS], Option<usize>),
    flinches: &[(Location, Vec3, f32)],
    (ms, to_model): (f32, Quat),
    transforms: &mut impl Transforms,
) {
    let mut axes: Vec<(usize, Vec3)> = Vec::new();
    let mut add = |node: usize, axis: Vec3| match axes.iter_mut().find(|(at, _)| *at == node) {
        Some((_, sum)) => *sum += axis,
        None => axes.push((node, axis)),
    };
    for &(location, way, hit_ms) in flinches {
        let Some(node) = spots[location as usize] else {
            continue;
        };
        let axis = crate::bolt_hits::flinch_axis(&[(to_model * way, hit_ms)], ms);
        add(node, axis);
        let trunk = matches!(
            location,
            Location::None
                | Location::Chest
                | Location::ChestLeft
                | Location::ChestRight
                | Location::Back
                | Location::BackLeft
                | Location::BackRight
                | Location::Waist
        );
        if let Some(upper) = upper.filter(|&upper| trunk && upper != node) {
            add(upper, axis * crate::bolt_hits::TRUNK_SHARE);
        }
    }
    for (node, axis) in axes {
        if axis != Vec3::ZERO {
            let turn = crate::bolt_hits::capped_turn(axis);
            turn_in_model(transforms, (nodes, rest), node, turn);
        }
    }
}

/// Puts each node a bolt's flinch may turn (`spots` and `upper`, from
/// `Body::flinch_nodes`) back to its turn at rest (`rest`), unless the pose
/// wrote it this frame (`posed`).
fn unflinch(
    (nodes, rest): (&[Entity], &Rest),
    (spots, upper): ([Option<usize>; LOCATIONS], Option<usize>),
    posed: &[(usize, Quat)],
    transforms: &mut impl Transforms,
) {
    for node in spots.into_iter().chain([upper]).flatten() {
        if posed.iter().any(|&(at, _)| at == node) {
            continue;
        }
        if let (Some(&entity), Some(&(_, _, rotation))) = (nodes.get(node), rest.get(node)) {
            edit(transforms, entity, |t| t.rotation = rotation);
        }
    }
}

/// Turns `entity` by `turn` in the space of the first of `above` (its
/// ancestors, from there down), those posed as they stand.
fn turn_under(transforms: &mut impl Transforms, above: &[Entity], entity: Entity, turn: Quat) {
    let above = (above.iter())
        .filter_map(|&e| transforms.read(e).map(|t| t.rotation))
        .fold(Quat::IDENTITY, |above, rotation| above * rotation);
    edit(transforms, entity, |t| {
        t.rotation = (above.inverse() * turn * above * t.rotation).normalize();
    });
}

/// The held blade's flick across the bolt `at`'s holder turned last (see
/// `FLICK_MS`): where (game space, metres) and how far, at `ms`; the
/// deflections seen kept in `figure`.
fn flick(figure: &mut Figure, at: &Blended, ms: f64) -> Option<(Vec3, f32)> {
    if let Some(fighter) = &at.fighter {
        let seen = *figure
            .deflected
            .get_or_insert(fighter.next_event.wrapping_sub(1));
        let newest = fighter
            .events
            .iter()
            .flatten()
            .filter(|event| event.result == sim::combat::Outcome::Deflected)
            .filter(|event| event.sequence.wrapping_sub(seen).cast_signed() > 0)
            .filter_map(|event| Some((event.sequence, event.at?)))
            .max_by_key(|(sequence, _)| sequence.wrapping_sub(seen));
        if let Some((sequence, point)) = newest {
            figure.deflected = Some(sequence);
            figure.flick = Some((ms, Vec3::from_array(point.map(|v| v as f32)) * UNIT));
        }
    }
    let (start, point) = figure.flick?;
    let weight = (-((ms - start) as f32).max(0.0) / FLICK_MS).exp();
    if weight < 0.01 {
        figure.flick = None;
        return None;
    }
    Some((point, weight))
}

/// Where the eyes looking through them are (game space, metres): `eye` from
/// the eye height over the origin (`Cast::eye`).
fn eyes(at: &Blended, eye: Vec3) -> Vec3 {
    (Vec3::from_array(at.origin) + Vec3::Z * at.state.eye_height) * UNIT + eye
}

/// A point held before the eyes `eyes` (game space, metres): `ahead` along
/// the view's `forward`, `aside` along its `right`, and `down` straight
/// down. The drop is the world's, not the view's: looking straight down,
/// the view's down points back into the chest, so a gun or a blade held
/// there crowded the eye, seen end on with the arms folded around it
/// (issue 486).
fn before_eyes(eyes: Vec3, (forward, right): (Vec3, Vec3), [ahead, aside, down]: [f32; 3]) -> Vec3 {
    eyes + forward * ahead + right * aside - Vec3::Z * down
}

/// The view's forward, right and up (game space).
fn view_axes(at: &Blended) -> [Vec3; 3] {
    let (forward, right, up) = collision::trig::angle_vectors(at.view);
    [forward, right, up].map(Vec3::from_array)
}

/// A share eased toward `target` over `ease` seconds from its value as of
/// when (`share`, sim ms) to `ms`, then kept as of `ms`. The first one goes
/// straight to the target.
fn eased(share: &mut (f32, Option<f64>), (target, ease): (f32, f32), ms: f64) -> f32 {
    let (value, was) = *share;
    let seconds = was.map_or(1.0, |was| ((ms - was) / 1000.0).clamp(0.0, 0.25) as f32);
    let value = crate::gait::ease(value, target, seconds / ease);
    *share = (value, Some(ms));
    value
}

/// The saber move cuts: an attack, a special or a parry, not a start, a
/// return, a transition or none.
pub fn cutting(saber: &SaberState) -> bool {
    !saber.current_move.idle() && !saber.current_move.in_between()
}

/// The blade can touch something at `now` (sim ms): the saber move cuts
/// (`cuts`, `Figure::cuts`), or `saber` is blocked (a parry meeting a
/// blow), in a saber lock (`lock`, or `lock_until` not yet passed) or inside
/// a bolt block (`bolt_block_until` not yet passed). The move's own
/// `blocking` is not read: JA's ready stance blocks wide too, so it would
/// mark every lit frame.
pub fn in_contact(cuts: bool, saber: &SaberState, now: u64) -> bool {
    cuts || saber.blocked != Blocked::None
        || saber.lock.is_some()
        || saber.lock_until > now
        || saber.bolt_block_until > now
}

/// Puts a body where its character stands and, with `pose`, poses it, a
/// hand gripping a blade (while `armed`) reaching for it in `reach`
/// (`asset_ja::rig::Reach`) with the blades `wielded` holds, a gun out held
/// up and aimed (`gun_aim`), at the point under the crosshair (`sighted`)
/// for the player's own. Looked through (`eye`, `Cast::eye`), its gun
/// and its blade at rest are held up before its eyes. In a conversation a
/// head looks at the one its line pairs it with (`look`: where that one
/// stands, and the head's arc, `Head`) after the pose; as a line's speaker
/// it talks (`talking`) standing where no script's animation plays on it.
/// Each bolt that struck it lately (`flinches`: where, along which way in
/// game space, and when, view ms) turns a node of its body (`bolt_flinch`).
/// A script's animation shows on every body, a droid's or a beast's as a
/// humanoid's does, in a conversation or out of one, its clip's events
/// heard (`hear`). While one holds the world (`world_held`) it plays out
/// (`acting`), and a droid's or a beast's walk keeps to its stride. A
/// humanoid wholly on KOTOR's clips (nothing gripped or aimed) is KOTOR's
/// own clip on its model, its JA pose not made. That holds in a
/// conversation always. Out of one, it holds for a living body that no
/// blast is throwing and that is not the camera's own player (`own`), whose
/// JA pose its trace and the animation audit read. With no conversation
/// under way, it is otherwise drawn as it was before scene mode.
/// A droid or beast that leans with the floor stands on `floor`
/// (`ground_lean`). Under the duel rules (`rules`) a gripping hand turns its
/// blade the sim's way only while the blade is in contact
/// (`Figure::turn_share`).
/// A bolt that struck a body lately: where, from which way, and when.
type Flinch = (Location, Vec3, f32);
/// A grabber's hand held to its victim: which hand, where, and how much.
type GrappleTouch = (asset_ja::rig::Side, Vec3, f32);

#[allow(clippy::too_many_arguments)]
fn place<T: Transforms>(
    Member {
        body,
        figure,
        heard,
        ..
    }: &mut Member,
    at: &Blended,
    (soles, ms, eye, floor): (Vec3, f64, Option<Vec3>, Option<Floor>),
    (pose, armed, reach, own): (bool, bool, &[(Blade, Hand)], bool),
    (world_held, look, drawn, rules): (bool, Option<(Vec3, f32)>, Option<DrawPose>, SaberRules),
    (talking, flinches, touches): (Option<Acting>, &[Flinch], &[GrappleTouch]),
    (wielded, sighted): (Option<&Wielded>, Option<Vec3>),
    transforms: &mut T,
) -> Option<Posed> {
    let quarter = std::f32::consts::FRAC_PI_2;
    // A script's animation: moving, fighting or dying breaks it off. A
    // hold's plays whatever it does.
    let [x, y, _] = at.state.velocity;
    let ja_body = matches!(body.look, Look::Retargeted { .. });
    let held = at.fighter.as_ref().and_then(|f| held_clip(f, ms, ja_body));
    // Guns in hand or on their way to or from the holsters (`draw_anim`)
    // keep the arms the pose's.
    let readying = drawn.is_some_and(|drawn| drawn.in_hand || drawn.stow > 0.0);
    // An item's use (`sim::combat::items::clips`) plays over a drawn
    // weapon too: a shield's band is switched on in the middle of a fight.
    let using = at.fighter.is_some_and(|f| f.pack.using.is_some());
    let moving = (x * x + y * y).sqrt() >= STILL_SPEED;
    let acting = at
        .fighter
        .as_ref()
        .and_then(|f| drawn_acting(f, moving, ms, ja_body));
    // A speaker talks standing (`acting::or_talk`); a character has none.
    let talk = talking.filter(|_| !moving && at.alive && at.fighter.is_some());
    // One that hides the equipped items (Security, a mine's) puts the gun
    // away as it begins: the gun going does not break it off.
    let hiding = acting.is_some_and(|acting| acting::hides_items(&acting, ms));
    let still = held.is_some()
        || (x * x + y * y).sqrt() < STILL_SPEED
            && at.alive
            && (hiding || using || !armed && !readying);
    if let Some(playing) = &mut body.playing {
        playing.clear();
    }
    match &mut body.look {
        Look::Retargeted {
            rig,
            gripped,
            acted,
            shows,
            throw,
            draw,
            flinch,
            gait,
        } => {
            // Holding the blade out, the hand grips it.
            let hand_out = at.fighter.map_or(0, |f| f.hand.min(1));
            let gripping = gripped[usize::from(hand_out)].as_ref().filter(|_| armed);
            let rig = gripping.unwrap_or(&*rig);
            let acting = acting::or_talk(acting, talk, (acted, figure.breath.wield), ms);
            shows.follow(acting, (still, moving, at.alive, world_held), ms);
            hear(heard, shows, (acted, figure.breath.wield), ms);
            throw.follow(at.fighter.as_ref(), figure.launcher);
            // A player's own body (its gait) keeps JA's; JA's own pain,
            // drawn on the pose, is the flinch where the sim plays it.
            let pained = gait.is_some() || pained(&at.state);
            flinch.follow(at.fighter.as_ref(), pained, false);
            let mut shown: Vec<_> = shows
                .shown(
                    acted,
                    ms,
                    (
                        figure.breath.wield,
                        (armed && at.alive).then_some(figure.wield),
                    ),
                )
                .collect();
            let wield = figure.breath.wield;
            shown.extend(
                draw.shown(acted, ms, acting::draw_of(wield))
                    .into_iter()
                    .chain(flinch.shown(acted, ms, wield))
                    .filter(|_| at.alive),
            );
            // Out of a fight, its own clips (`gait`) unless a script's plays.
            let mut facing = None;
            if let Some(gait) = gait {
                let wanted = (at.alive && !armed && !readying && shown.is_empty())
                    .then(|| crate::gait::of_body(at.state.legs.anim, at.state.torso.anim))
                    .flatten();
                let legs = figure.yaw().unwrap_or(0.0);
                let motion = crate::gait::Motion {
                    speed: (x * x + y * y).sqrt() * UNIT,
                    travel: y.atan2(x).to_degrees(),
                    legs,
                };
                gait.follow(wanted, motion, ms);
                facing = gait.facing();
                shown.extend(gait.shown());
            }
            // A JA model faces its legs' yaw (in the pose); a KOTOR body,
            // which faces +Y, turns a quarter back to match. Under a KOTOR
            // clip, whose root faces +Y, the body turns to the yaw instead
            // (its gait's facing, while it shows).
            let faces = if shown.is_empty() {
                figure.yaw()
            } else {
                facing.or(figure.yaw())
            };
            let faces = faces.unwrap_or(0.0);
            let yaw = if shown.is_empty() {
                0.0
            } else {
                faces.to_radians()
            };
            edit(transforms, body.root, |t| {
                *t = Transform::from_translation(soles)
                    .with_rotation(Quat::from_rotation_z(yaw - quarter));
            });
            if !pose {
                return None;
            }
            at_rest((&body.nodes, &rig.rest, &rig.held), transforms);
            let face = body.face.as_ref();
            if let Some(face) = face {
                face.stand((ms, at.alive), transforms);
            }
            // Each KOTOR clip over the body, and over its face.
            let lay = |shown: &mut dyn Iterator<Item = (&Animator, f32, f32)>,
                       transforms: &mut T| {
                for (clip, seconds, weight) in shown {
                    clip.pose_over(seconds, weight, transforms);
                    if let Some(face) = face {
                        face.follow(clip, (seconds, weight), transforms);
                    }
                }
            };
            if let Some(playing) = &mut body.playing {
                let shown = shown.iter().map(|&(clip, at, _)| (clip.clip().clone(), at));
                playing.extend(shown);
            }
            // The model's space: the body faces +Y, turned a quarter back.
            let to_model = Quat::from_rotation_z(quarter - yaw);
            figure.blade = gripping.is_some();
            // Looking through its eyes, a blade at rest is held up before
            // them (coming back to it as a swing returns), the sim's blade
            // taken back as the next starts.
            let resting = eye.is_some()
                && at.alive
                && at.fighter.is_some_and(|f| {
                    let next = f.saber.current_move;
                    next.idle() || next.ret()
                });
            let frame_ms = figure.held_ms.map_or(0.0, |was| (ms - was).max(0.0)) as f32;
            figure.held_ms = Some(ms);
            let wanted = if resting { 1.0 } else { 0.0 };
            figure.held += (wanted - figure.held) * (1.0 - (-frame_ms / BLADE_EASE_MS).exp());
            let flick = flick(figure, at, ms);
            let held = eye.filter(|_| figure.held > 1e-3).map(|eye| {
                let [forward, right, up] = view_axes(at);
                let side = |hand: Hand| if hand == Hand::Left { -1.0 } else { 1.0 };
                let [ahead, aside, down] = BLADE_HOLD;
                let [rise_ahead, rise_aside, rise_up] = BLADE_RISE;
                move |hand: Hand| {
                    let hilt = before_eyes(
                        eyes(at, eye),
                        (forward, right),
                        [ahead, aside * side(hand), down],
                    );
                    let rise =
                        forward * rise_ahead + right * (rise_aside * side(hand)) + up * rise_up;
                    let rise = rise.normalize();
                    let rise = match flick {
                        Some((point, weight)) => rise
                            .lerp((point - hilt).normalize_or(rise), weight)
                            .normalize_or(rise),
                        None => rise,
                    };
                    (to_model * (hilt - soles), to_model * rise)
                }
            });
            // Each hand's blades paired with the sim's in it: the middle of
            // their starts reaches the middle of the sim's (a staff's), the
            // first along the sim's first; held before the eyes at rest.
            let mut reached = Vec::new();
            // A double-bladed hilt keeps the old reach in a guard: turned
            // the sim's way, its second blade passes through the KOTOR arm.
            let staff = wielded.is_some_and(|w| w.blades_in(hand_out, Hand::Right).len() > 1);
            figure.guarding &= !staff;
            let mut share = figure.reach_share(ms);
            if audit_off("reach") {
                share = 0.0;
            }
            // Under JA's rules the blades always turn the sim's way.
            // The lead-in and the contact are both read from the snapshot
            // `at` is (`Blended::taken_ms`), so the turn never sees the end
            // of a lead-in before its cut.
            let turn = if audit_off("turn") {
                0.0
            } else if rules.duel() {
                let saber = at.fighter.as_ref().map(|f| &f.saber);
                let contact = saber
                    .is_some_and(|saber| in_contact(cutting(saber), saber, ms.max(0.0) as u64));
                // An attack's lead-in lasts as long as the torso's timer
                // holds its move, from when the snapshot was taken.
                let lead = saber
                    .filter(|saber| saber.current_move.start() || saber.current_move.transition())
                    .map(|_| {
                        (at.state.torso.timer.max(0) as f64 + at.taken_ms - ms).max(0.0) as f32
                    });
                figure.turn_share((contact, lead), ms)
            } else {
                // Kept whole, so a change to the duel rules starts from it.
                figure.turn = (1.0, Some(ms));
                1.0
            };
            for hand in [Hand::Right, Hand::Left]
                .into_iter()
                .filter(|_| gripping.is_some())
            {
                let sims: Vec<&Blade> =
                    reach.iter().filter(|r| r.1 == hand).map(|r| &r.0).collect();
                let ends = wielded.map_or_else(Vec::new, |w| w.blades_in(hand_out, hand));
                let pairs = sims.len().min(ends.len());
                let (Some(first), Some(&(_, along))) = (sims.first(), ends.first()) else {
                    continue;
                };
                let mean = |points: &mut dyn Iterator<Item = Vec3>| {
                    points.take(pairs).sum::<Vec3>() / pairs as f32
                };
                let muzzle = mean(&mut sims.iter().map(|b| Vec3::from_array(b.muzzle) * UNIT));
                let (mut muzzle, mut direction) = (
                    to_model * (muzzle - soles),
                    to_model * Vec3::from_array(first.direction),
                );
                let most = if figure.locked {
                    LOCK_REACH
                } else {
                    asset_ja::rig::REACH
                };
                let (mut most, mut free, mut turn) = (most, 0.0, turn);
                if let Some(held) = &held {
                    let (hilt, rise) = held(hand);
                    let w = figure.held;
                    muzzle = muzzle.lerp(hilt, w);
                    direction = direction.normalize_or_zero().lerp(rise, w);
                    most += (BLADE_REACH - most) * w;
                    free = w;
                    // The blade held before the eyes points where it is held.
                    turn = 1.0 - (1.0 - turn) * (1.0 - w);
                }
                reached.push(Reach {
                    grip: hand as usize,
                    muzzle,
                    direction,
                    origin: mean(&mut ends.iter().map(|end| end.0)),
                    along,
                    share,
                    most,
                    turn,
                    free,
                    carries: figure.guarding,
                });
            }
            let gun = wielded.filter(|_| gripping.is_none());
            let aims = gun_aim(
                figure,
                at,
                (gun, armed, drawn),
                ms,
                (to_model, soles, eye, sighted),
            );
            // What the first gun held up aims at, where the head looks down
            // it; not looked through, where the eye hangs on the head.
            let sight = (aims.iter())
                .find(|aim| aim.stow.is_none_or(|stow| stow.share < 1e-3))
                .map(|aim| aim.target)
                .filter(|_| eye.is_none());
            // The JA pose's share left under the clips laid over it in turn.
            let under: f32 = shown.iter().map(|&(_, _, weight)| 1.0 - weight).product();
            // Its head stands level under the JA pose, heading where JA's
            // head heads (once the pose is made), let go as it dies, then
            // turns to whom it looks at in a conversation.
            let level = Hold {
                heading: None,
                share: under,
                alive: at.alive,
                crouched: crouches(&at.state),
                armed,
            };
            let head = |figure: &mut Figure, transforms: &mut T, level: Hold| {
                if let Some(bone) = &body.head {
                    let target = look.map(|look| look_toward(soles, faces, look));
                    // Aiming, it leans its head toward the gun side about
                    // the way its legs face. Not the body looked through,
                    // whose eye hangs on its head.
                    let legs = figure.yaw().unwrap_or(0.0).to_radians();
                    let forward = to_model * Vec3::new(legs.cos(), legs.sin(), 0.0);
                    let tilt = eye.is_none().then(|| (forward, figure.head_tilt(ms)));
                    // A raised gun: it looks down the gun at what the
                    // gun aims at.
                    // The cheek rides back with the stock as the shot
                    // pushes the gun.
                    let back = aims.first().map_or(0.0, |aim| aim.kick.1 * aim.weight);
                    // Where the rifle's seat in the shoulder stands, and
                    // which way the rifle's top faces, once the rifle is
                    // fully up in the shoulder.
                    let seat = aims.first().and_then(|aim| {
                        let (seat, _) = aim
                            .butt
                            .filter(|&(_, raised)| raised.min(aim.weight) >= SEATED)?;
                        let line = gun_lines(rig, std::slice::from_ref(aim)).pop()?;
                        let gun =
                            placed_in_model(transforms, (&body.nodes, &rig.rest), line.weapon)?;
                        Some((
                            gun.transform_point(seat),
                            (gun.rotation * aim.up).normalize(),
                        ))
                    });
                    let weld = aim_pose::cheek_weld_deg(aim_pose::wield_of(figure.breath.wield))
                        .map(|weld| (weld, back, seat));
                    let sight = sight.map(|point| (point, figure.raise_share(ms), weld));
                    figure
                        .head
                        .turn((target, ms), (Some(level), tilt, sight), bone, transforms);
                }
            };
            // A body a blast throws keeps its JA pose, which its ragdoll
            // starts from (`as_drawn_dead`).
            let whole = under <= 1.0 - WHOLE && gripping.is_none() && aims.is_empty();
            let thrown = blasted(at, ms).is_some();
            // A body a bolt struck lately is posed in full too, so that it
            // flinches (`bolt_flinch`).
            let struck = !flinches.is_empty();
            if whole && (world_held || (at.alive && !own && !thrown && !struck)) {
                // The JA pose comes back without easing from where it was
                // before the clips took the body over.
                figure.eased = Eased::default();
                figure
                    .breath
                    .follow(acted, false, (ms, stance::phase(body.root)));
                lay(
                    &mut shown.into_iter().chain(throw.shown(acted, ms)),
                    transforms,
                );
                head(figure, transforms, level);
                return None;
            }
            let ja = figure.pose_facing(ms, shown.is_empty());
            // Under JA's rules the hand always turns with the hilt.
            let in_fist = if rules.duel() && !audit_off("fist") {
                figure.duel_share(ms)
            } else {
                0.0
            };
            // A grapple finisher's grabber holds its victim where JA's
            // does (`grapple_view`).
            let touches = crate::grapple_view::in_model(touches, to_model, soles);
            let posed = rig.retargeter.pose(
                &ja,
                Vec3::ZERO,
                (&reached, &aims, &touches),
                (&mut figure.eased, ms as f32),
                in_fist,
            );
            for &(node, rotation) in &posed.rotations {
                if let Some(&entity) = body.nodes.get(node) {
                    edit(transforms, entity, |t| t.rotation = rotation);
                }
            }
            for &(node, offset) in posed.root.iter().chain(&posed.weapons) {
                if let (Some(&entity), Some(&(_, rest, _))) =
                    (body.nodes.get(node), rig.rest.get(node))
                {
                    edit(transforms, entity, |t| {
                        t.translation = rest + rig.animation_scale * offset;
                    });
                }
            }
            // The pose above writes only the bones JA's skeleton maps. A node
            // a bolt's flinch turns that it does not map (a player's head
            // hangs at `headhook`) goes back to rest, so the flinch's turn
            // from the frame before does not stay and add up.
            unflinch(
                (&body.nodes, &rig.rest),
                body.flinch_nodes,
                &posed.rotations,
                transforms,
            );
            // The guns held up: the breathing and the firing motion below
            // turn the chest under them, and a raised gun is turned back
            // onto what it aims at (`reaim`). Not the body looked through,
            // whose gun is held up before the eye.
            let lines = if eye.is_none() {
                gun_lines(rig, &aims)
            } else {
                Vec::new()
            };
            let rifle = aim_pose::wield_of(figure.breath.wield) == aim_pose::Wield::Rifle;
            let reaimed = |figure: &Figure, transforms: &mut T| {
                reaim(
                    &lines,
                    (&body.nodes, &rig.rest),
                    figure.raise_share(ms),
                    transforms,
                );
            };
            // Standing in one of JA's held stances, it breathes (`stance`);
            // not looked through, where the eye hangs on its head and its
            // weapon is held before the eye.
            let standing = (x * x + y * y).sqrt() < STILL_SPEED;
            let breathing =
                figure.holding && at.alive && standing && shown.is_empty() && eye.is_none();
            let phase = stance::phase(body.root);
            if let Some(breath) = figure.breath.follow(acted, breathing, (ms, phase)) {
                let model = rig.model();
                let named =
                    |node: usize, name: &str| model.nodes[node].name.eq_ignore_ascii_case(name);
                if let Some((clip, seconds, weight)) = breath.clip {
                    let turned: Vec<Entity> = posed
                        .rotations
                        .iter()
                        .filter(|&&(node, _)| stance::NODES.iter().any(|name| named(node, name)))
                        .filter_map(|&(node, _)| body.nodes.get(node).copied())
                        .collect();
                    clip.add_over(seconds, weight, &turned, transforms);
                }
                // The lean, about the way the legs face.
                let legs = figure.yaw().unwrap_or(0.0).to_radians();
                let forward = to_model * Vec3::new(legs.cos(), legs.sin(), 0.0);
                if let Some(node) = (0..model.nodes.len()).find(|&node| named(node, stance::LEANS))
                {
                    turn_in_model(
                        transforms,
                        (&body.nodes, &rig.rest),
                        node,
                        Quat::from_axis_angle(forward, breath.lean),
                    );
                }
            }
            // A pistol's arms follow its kick up after this, so only the
            // breathing is taken off its line. A rifle is turned back after
            // the firing motion's lean in sustained fire and after the head
            // is held level, whose straightened spine carries the arms a few
            // centimetres.
            if !rifle {
                reaimed(figure, transforms);
            }
            let firing = (gun, at.alive, eye.is_some());
            recoil(
                figure,
                transforms,
                (&body.nodes, rig),
                firing,
                (ms, to_model),
            );
            lay(
                &mut shown.into_iter().chain(throw.shown(acted, ms)),
                transforms,
            );
            let heading = ja_head_heading(&figure.g2, &ja);
            head(figure, transforms, Hold { heading, ..level });
            if rifle {
                reaimed(figure, transforms);
            }
            // What turned after the arms were posed moved a two-handed gun
            // from under the support hand: the hand takes it again. Not the
            // body looked through, whose gun is held up before the eye.
            if let ([aim], None) = (aims.as_slice(), eye) {
                regrip(rig, aim, &body.nodes, transforms);
            }
            // The bolts' flinches, over the breathing and the pain clip.
            bolt_flinch(
                (&body.nodes, &rig.rest),
                body.flinch_nodes,
                flinches,
                (ms as f32, to_model),
                transforms,
            );
            Some(posed)
        }
        Look::Ja => {
            // The pose as it is, the model at the origin, its −Y turned to
            // +X as the cgame's root matrix turns it.
            edit(transforms, body.root, |t| {
                *t = Transform::from_translation(soles + Vec3::Z * asset_ja::ORIGIN_HEIGHT)
                    .with_rotation(Quat::from_rotation_z(quarter));
            });
            if pose {
                for (bone, (rotation, translation)) in figure.pose(ms).into_iter().enumerate() {
                    if let Some(&entity) = body.nodes.get(bone + 1) {
                        edit(transforms, entity, |t| {
                            t.rotation = rotation;
                            t.translation = translation;
                        });
                    }
                }
            }
            None
        }
        Look::Clips(clips) => {
            let wield = figure.breath.wield;
            let acting = acting::or_talk(acting, talk, (&clips.acted, wield), ms);
            clips
                .shows
                .follow(acting, (still, moving, at.alive, world_held), ms);
            clips.flinch.follow(at.fighter.as_ref(), false, true);
            clips.attack.follow(at.fighter.as_ref());
            clips.throw.follow(at.fighter.as_ref(), figure.launcher);
            // Its scripted clips show and sound as a retargeted body's do,
            // whether or not a conversation holds the world.
            hear(
                heard,
                &mut clips.shows,
                (&clips.acted, figure.breath.wield),
                ms,
            );
            // KOTOR models face +Y: yaw 0 (+X) is a quarter turn back. One
            // that leans with the floor is turned to it (`ground_lean`).
            let faces = figure.yaw().unwrap_or(at.view[1]);
            let (stands, leans) = clips
                .lean
                .as_mut()
                .map_or((soles, Quat::IDENTITY), |lean| lean.stand(floor, soles, ms));
            edit(transforms, body.root, |t| {
                *t = Transform::from_translation(stands)
                    .with_rotation(leans * Quat::from_rotation_z(faces.to_radians() - quarter));
            });
            if pose {
                at_rest((&body.nodes, &clips.rest, &clips.held), transforms);
                let playing = body.playing.as_mut();
                let face = body.face.as_ref();
                clips.pose(
                    figure,
                    at,
                    (ms, world_held, armed),
                    transforms,
                    playing,
                    face,
                );
                if let Some(bone) = &body.head {
                    let target = look.map(|look| look_toward(soles, faces, look));
                    figure
                        .head
                        .turn((target, ms), (None, None, None), bone, transforms);
                }
                let to_model =
                    (leans * Quat::from_rotation_z(faces.to_radians() - quarter)).inverse();
                bolt_flinch(
                    (&body.nodes, &clips.rest),
                    body.flinch_nodes,
                    flinches,
                    (ms as f32, to_model),
                    transforms,
                );
            }
            None
        }
    }
}

/// Whether JA's pain plays on `state`'s torso (the sim's flinch).
fn pained(state: &MotionState) -> bool {
    use sim::motion::Anim;
    matches!(
        state.torso.anim,
        Anim::BOTH_PAIN1 | Anim::BOTH_PAIN2 | Anim::BOTH_PAIN3
    )
}

/// The events the clip `shows` plays passed at `ms` that bring something
/// (`clip_events`), into `heard`.
fn hear(
    heard: &mut Vec<crate::clip_events::Heard>,
    shows: &mut acting::Shows,
    (acted, wield): (&acting::Bound, u8),
    ms: f64,
) {
    let passed = shows.passed(acted, ms, wield);
    heard.extend(passed.into_iter().filter_map(crate::clip_events::Heard::of));
}

impl Clips {
    /// Poses the body in the clip it plays at `ms` (`at`, `pose_own`), a
    /// script's animation laid over it as it shows (`acting::Shows`), as a
    /// humanoid's is: a droid's weld, a beast's sleep, and a weaponless
    /// hit's flinch. The clips it was posed in and how far into each
    /// (seconds) go to `playing`, where its effects want them. Its hung
    /// head's `face` follows them.
    fn pose(
        &mut self,
        figure: &Figure,
        at: &Blended,
        (ms, held, armed): (f64, bool, bool),
        transforms: &mut impl Transforms,
        mut playing: Option<&mut Vec<(Arc<AnimationIr>, f32)>>,
        face: Option<&crate::face::Face>,
    ) {
        self.pose_own(figure, at, (ms, held, armed), transforms);
        let own = self
            .shown
            .and_then(|(clip, at)| Some((self.clip(clip)?, at)));
        if let Some(face) = face {
            face.stand((ms, at.alive), transforms);
            if let Some((own, at)) = own {
                face.follow(own, (at, 1.0), transforms);
            }
        }
        if let (Some(playing), Some((own, at))) = (playing.as_deref_mut(), own) {
            playing.push((own.clip().clone(), at));
        }
        let wield = figure.breath.wield;
        let [x, y, _] = at.state.velocity;
        let moving = (x * x + y * y).sqrt() >= STILL_SPEED;
        let attack = self
            .attack
            .shown(&self.acted, ms, (wield, moving))
            .filter(|_| at.alive);
        let flinch = self
            .flinch
            .shown(&self.acted, ms, wield)
            .filter(|_| at.alive);
        let throw = self.throw.shown(&self.acted, ms);
        for (clip, seconds, weight) in self
            .shows
            .shown(&self.acted, ms, (wield, None))
            .chain(attack)
            .chain(flinch)
            .chain(throw)
        {
            clip.pose_over(seconds, weight, transforms);
            if let Some(face) = face {
                face.follow(clip, (seconds, weight), transforms);
            }
            if let Some(playing) = playing.as_deref_mut() {
                playing.push((clip.clip().clone(), seconds));
            }
        }
    }

    /// Poses the body in its own clip at `ms` (`at`); a clip that takes over
    /// from another fades in from where the other stopped over its
    /// transition time (`Animator::transition`), as KOTOR blends them.
    fn pose_own(
        &mut self,
        figure: &Figure,
        at: &Blended,
        (ms, held, armed): (f64, bool, bool),
        transforms: &mut impl Transforms,
    ) {
        let Some((clip, seconds)) = self.at(figure, at, (ms, held, armed)) else {
            return;
        };
        if let Some((old, old_at)) = self.shown.filter(|&(old, _)| old != clip) {
            self.fading = Some((old, old_at, ms));
        }
        self.shown = Some((clip, seconds));
        let Some(animator) = self.clip(clip) else {
            return;
        };
        let fade = self.fading.and_then(|(old, old_at, since)| {
            let toward = ((ms - since) as f32 / 1000.0) / animator.transition();
            let from = self.clip(old)?;
            (toward < 1.0).then_some((from, old_at, toward.max(0.0)))
        });
        match fade {
            Some((from, from_at, toward)) => {
                animator.pose_from(seconds, (from, from_at), toward, transforms);
            }
            None => animator.pose(seconds, transforms),
        }
        if fade.is_none() {
            self.fading = None;
        }
    }

    /// Its clip `index`: one of its own (`PAUSE` to `GETUP`), a wield's
    /// ready stance (`READY_AT`), or its walk or run with a rifle
    /// (`RIFLE_AT`).
    fn clip(&self, index: usize) -> Option<&Animator> {
        let acted = |wanted: u16| {
            let (_, clip) = self.acted.iter().find(|(a, _)| *a == wanted)?;
            Some(clip)
        };
        if let Some(gait) = index.checked_sub(RIFLE_AT) {
            return acted(acting::RIFLE_GAIT + u16::try_from(gait).ok()?);
        }
        match index.checked_sub(READY_AT) {
            None => self.clips[index].as_ref(),
            Some(wield) => acted(acting::READY + u16::try_from(wield).ok()?),
        }
    }

    /// Which of its walk (0) and run (1) cycles clip `index` plays: its own
    /// walk or run, or the same with a rifle.
    fn cycle_of(index: usize) -> Option<usize> {
        match index {
            WALK | RUN => Some(index - WALK),
            _ => index.checked_sub(RIFLE_AT).filter(|gait| *gait < 2),
        }
    }

    /// How long its dying clip lasts (ms).
    fn die_ms(&self) -> Option<f64> {
        let die = self.clips[DIE].as_ref()?;
        Some(f64::from(die.length()) * 1000.0)
    }

    /// The clip shown and the one fading out, by name, and how much of each
    /// scripted clip shows over them at `ms` (by its wield number `wield`),
    /// for dumps.
    fn facts(&self, ms: f64, wield: u8) -> serde_json::Value {
        let name =
            |clip: Option<usize>| clip.and_then(|clip| Some(self.clip(clip)?.name().to_string()));
        let acted: Vec<_> = (self.shows.shown(&self.acted, ms, (wield, None)))
            .map(|(clip, _, weight)| (clip.clip().name.clone(), weight))
            .collect();
        serde_json::json!({
            "shown": name(self.shown.map(|shown| shown.0)),
            "acted": acted,
            "fading_from": name(self.fading.map(|fading| fading.0)),
            "attacks": self.attack.count,
            "flinches": self.flinch.count,
        })
    }

    /// The clip a creature plays at `ms` and how far into it (seconds):
    /// dead, its dying clip from its death to the end, then its dead clip
    /// looped (or the dying clip held); alive, standing, walking or running,
    /// looped on the clock, but while the world is held (`held`, S7) walking
    /// and running by the distance covered, a cycle a stride
    /// (`gait::by_stride`), as a humanoid's gait does; standing with its
    /// weapons readied (`armed`), its wield's ready stance where it has one.
    fn at(
        &mut self,
        figure: &Figure,
        at: &Blended,
        (ms, held, armed): (f64, bool, bool),
    ) -> Option<(usize, f32)> {
        let seconds = (self.cycled_ms).map_or(0.0, |was| ((ms - was) / 1000.0).clamp(0.0, 0.25));
        self.cycled_ms = Some(ms);
        let [x, y, _] = at.state.velocity;
        let speed = (x * x + y * y).sqrt();
        for (cycle, clip) in [WALK, RUN].into_iter().enumerate() {
            if let Some(length) = (self.clips[clip].as_ref()).map(Animator::length) {
                let by = crate::gait::by_stride(
                    speed * UNIT,
                    seconds as f32,
                    self.strides[cycle],
                    length,
                );
                self.cycles[cycle] = (self.cycles[cycle] + by).rem_euclid(length.max(1e-3));
            }
        }
        let length = |clip: usize| self.clip(clip).map(Animator::length);
        let looped = |clip: usize, since: f64| -> Option<(usize, f32)> {
            let seconds = ((ms - since) / 1000.0).max(0.0) as f32;
            Some((clip, seconds % length(clip)?))
        };
        if !at.alive {
            let since = figure.died.map_or(ms, |died| died as f64);
            let dying = ((ms - since) / 1000.0).max(0.0) as f32;
            return match (length(DIE), length(DEAD)) {
                (Some(die), _) if dying < die => Some((DIE, dying)),
                (Some(die), None) => Some((DIE, die)),
                (die, Some(_)) => looped(DEAD, since + f64::from(die.unwrap_or(0.0)) * 1000.0),
                (None, None) => None,
            };
        }
        // Up again (a revive, the fight's end): its get-up from the dead
        // clip, once.
        if let Some(rose) = figure.rose
            && let Some(getup) = length(GETUP)
        {
            let up = ((ms - rose as f64) / 1000.0).max(0.0) as f32;
            if up < getup {
                return Some((GETUP, up));
            }
        }
        let legs = at.state.legs.anim.name();
        let walking = legs.contains("WALK") || (speed < RUN_SPEED && !legs.contains("RUN"));
        let either = |first: usize, second: usize| {
            [first, second]
                .into_iter()
                .find(|&clip| self.clip(clip).is_some())
        };
        // Standing in a fight, a body that is not a creature model holds its
        // wield's ready stance (reone's pause in combat, `g<wield>r1`).
        let ready = READY_AT + usize::from(figure.breath.wield);
        let clip = if speed < STILL_SPEED {
            match armed && at.fighter.is_some() && !self.creature {
                true => either(ready, PAUSE),
                false => either(PAUSE, PAUSE),
            }
        } else if walking {
            either(WALK, RUN)
        } else {
            either(RUN, WALK)
        };
        // Walking or running in a fight with a rifle readied, a body that is
        // not a creature model holds the rifle up (`walkrf`, `runrf`), as
        // KOTOR's walk and run with a rifle do.
        let rifle = armed
            && at.fighter.is_some()
            && !self.creature
            && figure.breath.wield == acting::RIFLE_WIELD;
        let clip = clip.map(|clip| {
            match clip {
                WALK | RUN if rifle => {
                    Some(RIFLE_AT + clip - WALK).filter(|&held_up| self.clip(held_up).is_some())
                }
                _ => None,
            }
            .unwrap_or(clip)
        });
        let clip = clip.or(either(PAUSE, PAUSE))?;
        match Self::cycle_of(clip) {
            Some(cycle) if held => Some((clip, self.cycles[cycle])),
            _ => looped(clip, 0.0),
        }
    }
}

/// The scene's models by index: each one's root and node entities.
pub fn model_index(
    models: &Query<(Entity, &SceneModel)>,
    count: usize,
) -> Vec<Option<(Entity, Vec<Entity>)>> {
    let mut index = vec![None; count];
    for (entity, model) in models {
        if let Some(slot) = index.get_mut(model.index) {
            *slot = Some((entity, model.nodes.clone()));
        }
    }
    index
}
