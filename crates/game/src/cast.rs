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
//!   one, looped (reone's `die` and `dead`). A clip that takes over fades
//!   in from where the last one stopped over its transition time.
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
//! Limits, for the frame's cost: every character follows its snapshots and
//! steers every step (cheap); a pose (Ghoul2 and the retarget, or the
//! clip) is worked out every frame for the `POSED` characters nearest the
//! camera within `POSE_RANGE` (the camera's own player always), every
//! `FAR_EVERY`th frame in turn for the rest within `FAR_RANGE`; beyond it
//! they keep their last pose while they move. The torso's `Motion` bolt, which needs a pose
//! of its own, is read only for posed characters whose legs and torso play
//! different animations (the only time the look uses it). Each distinct
//! body model is lined up with JA's skeleton once and kept for the run.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_ja::angles;
use asset_ja::events::{Events, Keyed, Passage};
use asset_ja::g2::{Bones, Local, Part, PartSet, Player as G2};
use asset_ja::rig::{Aim, Grip, OffHand, Posed, Reach, Retargeter, Side, Stowed};
use asset_odyssey::anim::AnimationIr;
use asset_odyssey::hooks;
use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use collision::UNIT;
use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::Gla;
use script_ncs::vm::ObjectId;
use sim::combat::idle::Acting;
use sim::combat::sheet::{Blaster, Sheet};
use sim::combat::{Delivery, Fighter, Location, Weapon};
use sim::module::ObjectKind;
use sim::motion::{self, Anim, MotionState};
use sim::script::dialogue::Conversation;
use sim::script::effects::Hold;
use sim::skeleton::{Blade, Hand};
use sim::{EntityState, Instances, Kind, Seat, SimId, Snapshot};

use crate::acting;
use crate::draw_anim::DrawPose;
use crate::holdings::{self, Holdings};
use crate::ragdolls::{self, Doll, Dying, Lengths, Ragdolls};
use crate::scene::{Animator, SceneModel};
use crate::stance;
use crate::viewer::{CreatureClips, CreatureView, LoadedScene};
use crate::weapons::{self, Armed, Hands, Hide, Owner, Props, Sabers, Stow, Wielded};

/// How many characters are posed every frame, nearest the camera first.
pub const POSED: usize = 16;
/// How far from the camera a character is posed every frame (metres).
pub const POSE_RANGE: f32 = 30.0;
/// The rest within `FAR_RANGE` (metres) take turns, each posed every
/// `FAR_EVERY`th frame: a held pose would stop a far fight mid-swing.
pub const FAR_RANGE: f32 = 150.0;
pub const FAR_EVERY: usize = 4;
/// Below this speed (units per second) a creature stands. It walks or runs
/// as the sim's legs animation says (as JA picks walk or run by the walk
/// button the scripts' moves press); with legs doing neither, below
/// `RUN_SPEED` (tuning, between JA's walk and run speeds) it walks.
const STILL_SPEED: f32 = 10.0;
const RUN_SPEED: f32 = 180.0;
/// How quickly a gun comes up to its aim (ms, the time constant): about
/// the blend JA's torso takes into its ready pose.
const AIM_EASE_MS: f32 = 60.0;
/// How long a shot's kick takes to peak (ms).
const KICK_RISE_MS: f32 = 25.0;
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
/// Seconds a gripping hand takes to settle into a held stance's reach:
/// longer, the stance's pose coming in after a return (at `REACH_EASE_S`
/// the hand snapped up to `asset_ja::rig::REACH` across the chest).
const STANCE_EASE_S: f32 = 0.3;
/// How much a gripping hand reaches for the sim's blade in a held stance.
const STANCE_REACH: f32 = 1.0;
/// Where a gun's muzzle is held before the eyes looking through them
/// (`first_person`): ahead, right and down along the view (metres; tuning,
/// the owner's approved look, as are the blade's below).
const HOLD: [f32; 3] = [0.5, 0.14, 0.18];
/// Where a blade at rest is held before them: the right hand's hilt ahead,
/// right and down (metres; the left's mirrored), the blade rising ahead,
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
    retargeter: Retargeter<'static>,
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
        }
    }

    /// The body with its scripted clips.
    pub fn with_acted(self, acted: Vec<(u16, Arc<AnimationIr>)>) -> BodyRig {
        BodyRig { acted, ..self }
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
                Grip::new(self.ir, rig.gla, bolt, blade, weapons::WEAPON_NODES[hand]).ok()
            })
            .collect::<Option<Vec<Grip>>>()?;
        // The retargeter borrows its grips for as long as the run lasts.
        let grips: &'static [Grip] = Box::leak(grips.into_boxed_slice());
        let hold = self.gripping_hold(double, dual);
        let gripped = BodyRig::with_grips(self.ir, rig, hold, grips);
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

/// The character `id` at `ms` in `history`, or, the own character, from
/// its predictions.
pub fn blended_for(
    history: &[Arc<Snapshot>],
    id: SimId,
    ms: f64,
    own: Option<&Own<'_>>,
) -> Option<Blended> {
    match own {
        Some(own) if own.id == id => {
            let mut at = blended(own.history, id, own.ms)?;
            at.origin = std::array::from_fn(|axis| at.origin[axis] + own.correction[axis]);
            Some(at)
        }
        _ => blended(history, id, ms),
    }
}

/// The character `id` at `ms` in `history` (oldest first), as drawn: one
/// a destroy took out of the world stands as it went while it departs.
pub fn blended(history: &[Arc<Snapshot>], id: SimId, ms: f64) -> Option<Blended> {
    let after = history.iter().position(|s| s.time_ms as f64 >= ms);
    let (a, b) = match after {
        Some(0) => (history.first()?, history.first()?),
        Some(i) => (&history[i - 1], &history[i]),
        None => (history.last()?, history.last()?),
    };
    let (ea, eb) = (a.drawn(id)?, b.drawn(id)?);
    let (pa, pb) = (ea.player?, eb.player?);
    let span = (b.time_ms - a.time_ms) as f64;
    let t = if span > 0.0 {
        ((ms - a.time_ms as f64) / span) as f32
    } else {
        1.0
    };
    let thrown = |e: &EntityState| e.fighter.and_then(|f| f.saber.thrown).map(|t| t.at);
    Some(Blended {
        state: pb,
        origin: lerp(pa.origin, pb.origin, t),
        view: std::array::from_fn(|i| pa.view[i] + angles::wrap_180(pb.view[i] - pa.view[i]) * t),
        alive: eb.fighter.is_none_or(|f| f.alive()),
        dead: eb.fighter.is_some_and(|f| f.dead()),
        fighter: eb.fighter,
        thrown_at: thrown(eb).map(|at| thrown(ea).map_or(at, |from| lerp(from, at, t))),
    })
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

/// The top of body model `body`'s meshes and those of the models hung on
/// it (its head), at rest, over its root (metres); none without meshes.
fn model_height(body: usize, scene: &LoadedScene) -> Option<f32> {
    let hung = scene
        .models
        .iter()
        .filter(|placed| placed.hook.is_some_and(|(on, _)| on == body))
        .map(|placed| {
            let attach = placed
                .attach
                .as_ref()
                .map_or(Mat4::IDENTITY, crate::scene::to_mat4);
            mesh_top(placed, attach)
        });
    std::iter::once(mesh_top(scene.models.get(body)?, Mat4::IDENTITY))
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
/// seat's own character while in the party, then the creatures that follow
/// the seat, by id; it is turned to start at the body (as KOTOR's leader
/// key turns it), so the next after the body is the next to lead.
pub fn party_of(snapshot: &Snapshot, seat: Seat) -> Vec<&EntityState> {
    let body = snapshot.body(seat);
    let own = snapshot.own(seat).filter(|own| snapshot.in_party(own));
    let followers = snapshot.entities.iter().filter(|entity| {
        matches!(entity.kind, Kind::Object(..))
            && entity
                .fighter
                .is_some_and(|fighter| fighter.mind.leader == Some(seat))
    });
    let mut party: Vec<&EntityState> = own.into_iter().chain(followers).collect();
    if let Some(body) = body {
        match party.iter().position(|member| member.id == body.id) {
            Some(at) => party.rotate_left(at),
            None => party.insert(0, body),
        }
    }
    party
}

/// JA's `WeaponReadyAnim` for the weapon: what the torso holds standing
/// ready (sabers and KOTOR's melee weapons, which fight with the saber
/// moves, `BOTH_STAND2`).
fn ready_anim(weapon: Weapon) -> &'static str {
    match weapon.delivery {
        Delivery::Saber(_) | Delivery::Melee => "BOTH_STAND2",
        Delivery::Blaster(Blaster::Pistol) => "TORSO_WEAPONREADY2",
        Delivery::Blaster(_) | Delivery::Baton | Delivery::Unarmed => "TORSO_WEAPONREADY3",
    }
}

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
    ready: &'static str,
    /// Its melee wield number (`Sheet::wield_number`): the ready stance it
    /// holds after a scene's round (`acting`).
    wield: u8,
    /// The legs' and the torso's whole frame at the last step, and the
    /// animation it was in.
    frames: [Option<(i32, Anim)>; 2],
    /// The torso as the snapshots last had it (animation, flip), and when
    /// each hand's gun last fired (sim ms; right, left): its kick runs from
    /// there.
    shown_torso: Option<(Anim, bool)>,
    shot_ms: [Option<f64>; 2],
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
    /// Its saber move cuts (an attack, a special, a parry; not a start, a
    /// return, a transition or none): only then do the hands reach for the
    /// sim's blades in full (`reach_share`).
    cuts: bool,
    breath: stance::Breath,
    /// Its left hand on a lone hilt (`Retargeter::pose`).
    off_hand: OffHand,
    /// How much its gripping hands reach for the sim's blades
    /// (`reach_share`), as of when (sim ms).
    reach: (f32, Option<f64>),
    /// How far its blade at rest is held up before its eyes (`place`), as
    /// of when.
    held: f32,
    held_ms: Option<f64>,
    /// Its hand grips a blade (a saber's or a melee weapon's) as posed.
    blade: bool,
    /// The last of its deflections seen (its event's sequence), and the
    /// held blade's flick across the newest: since when (ms) and where
    /// (game space, metres).
    deflected: Option<u16>,
    flick: Option<(f64, Vec3)>,
    /// Its head's turn toward whom it looks at in a conversation (S7).
    head: Head,
}

impl Figure {
    fn new(rig: &Rig, yaw: f32, ready: &'static str) -> Figure {
        let mut g2 = G2::new(rig.gla);
        g2.quirks = rig.quirks;
        Figure {
            bones: Bones::of(&g2),
            g2,
            look: angles::State::facing(yaw),
            legs: None,
            torso: None,
            adopted: [None; 2],
            turned: None,
            died: None,
            ready,
            wield: 2,
            frames: [None; 2],
            shown_torso: None,
            shot_ms: [None; 2],
            away: false,
            aim: [0.0; 2],
            drawing: crate::draw_anim::Drawing::default(),
            aim_ms: None,
            holding: false,
            cuts: false,
            breath: stance::Breath::default(),
            off_hand: OffHand::default(),
            reach: (1.0, None),
            held: 0.0,
            held_ms: None,
            blade: false,
            deflected: None,
            flick: None,
            head: Head::default(),
        }
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
            out.extend(keyed.iter().filter(|k| passage.passes(k.frame)));
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
    /// ready: the torso plays what the legs play.
    fn follow(&mut self, rig: &Rig, time_ms: u64, entity: &EntityState, armed: bool) {
        let Some(p) = entity.player else { return };
        match entity.fighter.map(|f| f.alive()) {
            Some(false) if self.died.is_none() => self.died = Some(time_ms),
            // Up again (a revive, the fight's end): its fall is over.
            Some(true) => self.died = None,
            _ => {}
        }
        let torso = (p.torso.anim, p.torso.flip);
        if self.shown_torso != Some(torso) {
            if matches!(torso.0, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3) {
                // The hands the shot left (`Shooting::hands`; one gun's is
                // the gun out's, the first).
                let hands = entity.fighter.map_or(1, |f| f.shooting.hands);
                for (hand, shot) in self.shot_ms.iter_mut().enumerate() {
                    if hands & 1 << hand != 0 || (hand == 0 && hands & 3 == 0) {
                        *shot = Some(time_ms as f64);
                    }
                }
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
        self.cuts = entity.fighter.is_some_and(|f| {
            let current = f.saber.current_move;
            !current.idle() && !current.in_between()
        });
        let posed = entity
            .fighter
            .filter(|f| f.alive() && !rig.quirks)
            .and_then(|f| Some([f.pose.legs?, f.pose.torso?]));
        if let Some(parts) = posed {
            self.adopt(parts, (away, keep), time_ms as f32);
            return;
        }
        self.adopted = [None; 2];
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
            let blend = was.map_or(0.0, |(old, _)| {
                asset_ja::blend_ms(Some(old.name()), now.0.name(), rig.quirks)
            });
            if let Some(sequence) = rig.sequences.get(now.0.name()) {
                self.g2.set_part(
                    &PartSet {
                        part,
                        sequence,
                        at_ms: time_ms as f32,
                        blends: (blend, blend),
                        speed_scale: 1.0,
                        before,
                        after,
                    },
                    self.bones,
                );
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
    /// played.
    fn adopt(
        &mut self,
        parts: [sim::combat::saber::blades::Shown; 2],
        (away, keep): (bool, bool),
        now_ms: f32,
    ) {
        let bones = [
            vec![self.bones.root],
            vec![self.bones.torso, self.bones.motion],
        ];
        let parts = if away { [parts[0], parts[0]] } else { parts };
        let switched = away != self.away;
        self.away = away;
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
            let blend_ms = if away {
                HOLSTER_BLEND_MS
            } else {
                DRAW_BLEND_MS
            };
            for bone in bones.into_iter().flatten() {
                if part == 1 && switched {
                    self.g2.adopt_blending(bone, &timed, now_ms, blend_ms);
                } else {
                    self.g2.adopt(bone, &timed);
                }
            }
        }
        let [legs, torso] = parts.map(|part| Some((part.anim, part.flip)));
        self.legs = legs;
        if !keep {
            self.torso = torso;
        }
    }

    /// The angles at `ms`, `frame_ms` after the last step (which advances
    /// their swinging), as JA's client turns them each rendered frame.
    /// `motion` reads the torso's `Motion` bolt, a pose's work.
    fn steer(&mut self, rig: &Rig, at: &Blended, ms: f64, frame_ms: f32, motion: bool) {
        let (Some((legs, _)), Some((torso, _))) = (self.legs, self.torso) else {
            return;
        };
        let motion = match self.bones.motion {
            Some(bone) if motion && legs != torso => {
                let pose = self.g2.pose(ms as f32);
                self.g2.bolt_angles(&pose, bone)
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
            legs: legs.name(),
            torso: torso.name(),
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
    /// (`cuts`), eased over `REACH_EASE_S`; `STANCE_REACH` while it stands in
    /// one of JA's held stances (`holding`), eased in over `STANCE_EASE_S`;
    /// none otherwise (a start, a return, a transition, a flip, a run:
    /// nothing is cut, and JA's arm is the truth). The blades always turn
    /// the sim's way.
    fn reach_share(&mut self, ms: f64) -> f32 {
        let (share, was) = self.reach;
        let seconds = was.map_or(1.0, |was| ((ms - was) / 1000.0).clamp(0.0, 0.25) as f32);
        let (target, ease) = match (self.cuts, self.holding) {
            (true, _) => (1.0, REACH_EASE_S),
            (false, true) => (STANCE_REACH, STANCE_EASE_S),
            (false, false) => (0.0, REACH_EASE_S),
        };
        let share = crate::gait::ease(share, target, seconds / ease);
        self.reach = (share, Some(ms));
        share
    }

    /// Its hand grips a blade as last posed.
    pub fn blade(&self) -> bool {
        self.blade
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
        let (rotation, translation) = self.g2.bone_final(&self.pose(ms), bolt.bone);
        Mat4::from_rotation_translation(rotation, translation) * crate::scene::to_mat4(&bolt.matrix)
    }

    /// The JA pose at `ms`, turned as the cgame turns it: the spine and
    /// neck by their angles, the whole model to the legs' yaw.
    pub fn pose(&self, ms: f64) -> Vec<Local> {
        self.pose_facing(ms, true)
    }

    /// `pose`, the whole model turned to the legs' yaw only if `facing`.
    fn pose_facing(&self, ms: f64, facing: bool) -> Vec<Local> {
        let mut pose = self.g2.pose(ms as f32);
        if let Some(turned) = &self.turned {
            for &(bone, angles) in &turned.bones {
                if let Some(bone) = self.g2.bone(bone) {
                    self.g2.turn(&mut pose, bone, angles);
                }
            }
            if let Some(root) = self.bones.root.filter(|_| facing) {
                let yaw = Quat::from_rotation_z(turned.legs_yaw.to_radians());
                pose[root] = (yaw * pose[root].0, yaw * pose[root].1);
            }
        }
        pose
    }
}

/// The KOTOR clips a creature plays, bound to its nodes (by `PAUSE`,
/// `WALK`, `RUN`, `DIE`, `DEAD`), the one it showed last and how far in, and
/// the one it left, fading out (where it stopped, and when, sim ms); its
/// clips for scripts' animations and what it shows of them (`acting`); the
/// metres its walk and run cycles cover and how far into each it is, as of
/// when (sim ms); its nodes' parents and rest (`rest_of`).
struct Clips {
    clips: [Option<Animator>; 5],
    shown: Option<(usize, f32)>,
    fading: Option<(usize, f32, f64)>,
    acted: acting::Bound,
    shows: acting::Shows,
    flinch: acting::Flinch,
    strides: [f32; 2],
    cycles: [f32; 2],
    cycled_ms: Option<f64>,
    rest: Vec<(Option<usize>, Vec3, Quat)>,
}

const PAUSE: usize = 0;
const WALK: usize = 1;
const RUN: usize = 2;
const DIE: usize = 3;
const DEAD: usize = 4;

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
    height: Option<f32>,
    /// Its model's index among the scene's models.
    model: usize,
    /// The node its head turns by in a conversation (`head_node`).
    head: Option<usize>,
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
    /// The paralysis holding it: until when (sim ms), and the moment its
    /// clips stopped at (`held_since`).
    paralysed: Option<(u64, f64)>,
}

impl Member {
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
/// character. The combat effects put a hit there.
#[derive(Resource, Default)]
pub struct Hitboxes(pub BTreeMap<SimId, [Option<Entity>; LOCATIONS]>);

/// Where each character's shots leave its guns as drawn (`Wielded::guns`):
/// each gun model's root and the muzzle in its space (metres), by
/// character and the hand a shot names (a pair's right and left; else the
/// gun out first). The combat effects start its flashes and bolts there.
#[derive(Resource, Default)]
pub struct Guns(pub BTreeMap<SimId, [Option<(Entity, Vec3)>; 2]>);

/// Each drawn character's body model's root, by character: the script
/// effects' glows cover its meshes (`script_fx`), the stealth shell its
/// body (`stealth_view`).
#[derive(Resource, Default)]
pub struct BodyRoots(pub BTreeMap<SimId, Entity>);

/// Where the players' bodies are among the scene's models: seat `s`'s at
/// `first + s × stride`, for `seats` seats.
#[derive(Clone)]
pub struct Bodies {
    pub first: usize,
    pub stride: usize,
    pub seats: u8,
    /// Seats dressed again in this scene (`wardrobe`): their bodies now.
    pub dressed: BTreeMap<Seat, usize>,
}

impl Bodies {
    fn of(&self, seat: Seat) -> Option<usize> {
        (seat.0 < self.seats).then(|| {
            self.dressed
                .get(&seat)
                .copied()
                .unwrap_or(self.first + usize::from(seat.0) * self.stride)
        })
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
    /// Who holds which weapon.
    holdings: Holdings,
    /// Each body model lined up once, by model name (and grip).
    rigs: BTreeMap<String, Arc<BodyRig>>,
    /// The characters posed this frame, how long placing and posing
    /// everyone took, and the part of it their weapons took.
    pub posed: usize,
    pub pose_ms: f64,
    pub arms_ms: f64,
    /// Frames posed so far: whose turn it is beyond the nearest.
    round: usize,
    /// The dead, falling and fallen.
    pub ragdolls: Ragdolls,
    /// Limbs cut off this frame, for the scene to part their bodies'
    /// meshes (`severing`): each body's nodes, its doll, the part.
    pub cuts: Vec<crate::severing::Owed>,
    /// The camera's player's eyes (game space, metres from its eye height
    /// over the origin) while it looks through them (`first_person`): its
    /// gun is held up before them.
    pub eye: Option<Vec3>,
    /// The game's base items' wields: the stance each weapon breathes in.
    wields: stance::Wields,
    /// The bodies the conversations under way hide items of (S8).
    unequipped: Vec<Unequipped>,
    /// The KOTOR clips each body posed plays this frame and how far into
    /// each (seconds), by its model: what keys its emitters and lights
    /// (`own_fx::BodyClips`).
    pub body_clips: BTreeMap<usize, Vec<(Arc<AnimationIr>, f32)>>,
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
            holdings: Holdings::default(),
            rigs: BTreeMap::new(),
            posed: 0,
            pose_ms: 0.0,
            arms_ms: 0.0,
            round: 0,
            ragdolls: Ragdolls::default(),
            cuts: Vec::new(),
            eye: None,
            wields,
            unequipped: Vec::new(),
            body_clips: BTreeMap::new(),
        }
    }

    /// A new world: every character starts over, the players on their
    /// bodies there (from `first_body` on). In the same scene (`kept`) each
    /// takes the weapons it held when it is drawn again
    /// (`Holdings::reset`). Blades' indices returned to retire.
    /// Seat `seat`'s body model, in the scene's models.
    pub fn body_of(&self, seat: Seat) -> Option<usize> {
        self.bodies.of(seat)
    }

    pub fn reset(
        &mut self,
        first_body: Option<usize>,
        kept: bool,
        commands: &mut Commands,
    ) -> Vec<usize> {
        if let Some(first) = first_body {
            self.bodies.first = first;
            self.bodies.dressed.clear();
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

    /// Every drawn character's guns (`Guns`).
    pub fn guns(&self) -> BTreeMap<SimId, [Option<(Entity, Vec3)>; 2]> {
        self.members
            .keys()
            .filter_map(|&id| {
                let guns = self.holdings.wielded(id)?.guns();
                guns.iter().any(Option::is_some).then_some((id, guns))
            })
            .collect()
    }

    /// Every drawn character's body root (`BodyRoots`).
    pub fn roots(&self) -> BTreeMap<SimId, Entity> {
        self.members
            .iter()
            .filter_map(|(&id, member)| Some((id, member.as_ref()?.body.root)))
            .collect()
    }

    pub fn figure(&self, id: SimId) -> Option<&Figure> {
        Some(&self.members.get(&id)?.as_ref()?.figure)
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
            let hands = self.fit((entity.id, carries), &mut body, scene, models, hitboxes);
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
                paralysed: None,
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
            let has = snapshot.own(seat);
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
            let body = match wanted {
                Some(view) => {
                    let droid = droid(entity, &objects);
                    self.body_in(view, droid, scene, models, commands)
                }
                None => member.own.take(),
            };
            if let Some(mut body) = body {
                let carries = matches!(entity.kind, Kind::Avatar(_));
                let hands = self.fit((id, carries), &mut body, scene, models, hitboxes);
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
    /// (`arms`, spawned later: a seat's by the armory, hidden; a creature
    /// made in play's with it, `made`): what it had is retired, their
    /// blades' indices returned. One not drawn yet takes them when it is
    /// (`Holdings::rearm`); one with nothing to draw it, never.
    pub fn rearm(
        &mut self,
        (owner, id): (Owner, Option<SimId>),
        sheet: &Sheet,
        arms: [Option<Armed>; 2],
        commands: &mut Commands,
    ) -> Vec<usize> {
        match id.and_then(|id| Some((id, self.members.get_mut(&id)?.take()))) {
            Some((_, None)) => holdings::retire(arms, commands),
            Some((id, Some(mut member))) => {
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
    /// returned to retire.
    pub fn forget(&mut self, (id, owner): (SimId, Owner), commands: &mut Commands) -> Vec<usize> {
        self.members.remove(&id);
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
                acted,
                strides,
            } = &view.clips;
            Look::Clips(Box::new(Clips {
                clips: [pause, walk, run, die, dead].map(bind),
                shown: None,
                fading: None,
                acted: acting::bind(placed, acted, nodes),
                shows: acting::Shows::default(),
                flinch: acting::Flinch::default(),
                strides: *strides,
                cycles: [0.0; 2],
                cycled_ms: None,
                rest: rest_of(&placed.ir),
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
            height: None,
            model,
            head: head_node(&placed.ir),
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
    /// else's stay in hand, `Hands::carries`).
    fn fit(
        &self,
        (id, carries): (SimId, bool),
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
        body.camera = node(hooks::CAMERA_HOOK).or_else(|| hung_camera(body.model, scene, models));
        body.height = model_height(body.model, scene);
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
        for entity in &snapshot.entities {
            if skip == Some(entity.id) || only.is_some_and(|only| only != entity.id) {
                continue;
            }
            if let Some(Some(member)) = self.members.get_mut(&entity.id) {
                let armed = entity.fighter.is_none_or(|f| {
                    weapons::armed(&f, snapshot.time_ms as f64, fought.contains(&entity.id))
                });
                let (out, held) = (weapon(entity, &objects), sheet(entity, &objects));
                member.figure.ready = ready_anim(out);
                member.figure.wield = held.wield_number();
                member.figure.breath.wield = stance::wield(out, &held, &self.wields);
                member
                    .figure
                    .follow(&self.rig, snapshot.time_ms, entity, armed);
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
            if let (Some(member), Some(at)) = (member, blended_for(history, id, ms, own)) {
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

    /// The characters to pose this frame: the camera's player, then the
    /// nearest others to `eye` (game space, metres) within `POSE_RANGE`,
    /// `POSED` in all; then, in turn, a `FAR_EVERY`th of the rest within
    /// `FAR_RANGE`.
    pub fn nearest(
        &mut self,
        history: &[Arc<Snapshot>],
        ms: f64,
        eye: Vec3,
        me: Option<SimId>,
    ) -> Vec<SimId> {
        let mut near: Vec<(f32, SimId)> = self
            .members
            .iter()
            .filter(|&(&id, member)| member.is_some() && Some(id) != me)
            .filter_map(|(&id, _)| {
                let at = blended(history, id, ms)?;
                let distance = (Vec3::from_array(at.origin) * UNIT).distance(eye);
                (distance <= FAR_RANGE).then_some((distance, id))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let every_frame = POSED - usize::from(me.is_some());
        let close = near
            .iter()
            .take(every_frame)
            .take_while(|(distance, _)| *distance <= POSE_RANGE)
            .count();
        self.round = self.round.wrapping_add(1);
        let round = self.round;
        // Turns by id, which stays put as the order by distance shifts.
        let far = near[close..]
            .iter()
            .map(|(_, id)| *id)
            .filter(|id| (id.0 as usize + round).is_multiple_of(FAR_EVERY));
        me.into_iter()
            .chain(near[..close].iter().map(|(_, id)| *id))
            .chain(far)
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
                Look::Retargeted { gait, .. } => (
                    "retargeted",
                    Some(serde_json::json!({
                        "gait": gait.as_ref().map(|gait| gait.facts()),
                        "breath": member.figure.breath.weight(),
                    })),
                ),
                Look::Ja => ("ja", None),
                Look::Clips(clips) => ("kotor clips", Some(clips.facts())),
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
                // A paralysis stopping its clips: until when, at what moment.
                "paralysed": member.paralysed,
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
            let hide = match (dlg.unequip_items, dlg.unequip_held) {
                (true, _) => Hide::All,
                (false, true) => Hide::Held,
                (false, false) => continue,
            };
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
        self.note_unequipped(newest, level);
        let looks = history
            .last()
            .map_or_else(Vec::new, |newest| looks(newest, level));
        let mut dying = Vec::new();
        self.body_clips.clear();
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
            if at.alive {
                self.ragdolls.rise(id);
            }
            let still = self.ragdolls.still(id);
            let ms = match own {
                Some(own) if own.id == id => own.ms,
                _ => ms,
            };
            // Stasis holds its pose where it took it, while it lasts.
            member.paralysed = held_since(at.fighter.as_ref(), member.paralysed, ms);
            let ms = member.paralysed.map_or(ms, |(_, since)| ms.min(since));

            let soles = soles(at.origin);
            let armed = at
                .fighter
                .is_none_or(|f| weapons::armed(&f, ms, fought.contains(&id)));
            let reach = reach
                .get(&id)
                .filter(|_| !self.rig.quirks)
                .map_or(&[][..], Vec::as_slice);
            // Only the dead fall as ragdolls: the downed must get up, and
            // one dying in place lies as its clip leaves it.
            let in_place = dying_in_place(&at);
            let falls = at.dead && member.body.doll.is_some() && !in_place;
            let death_plays = member.playing_death(ms);
            // Holstered guns drawn or put away as the body readies them
            // (`draw_anim`): only a body that carries them.
            let holsters = self
                .holdings
                .wielded(id)
                .map_or([None; 2], Wielded::holsters);
            let readied = armed && at.alive;
            let drawn = if holsters.iter().any(Option::is_some) {
                Some(member.figure.drawing.step(readied, ms))
            } else {
                // No holstered gun (another weapon, or none): the track
                // starts over, holstered, so nothing it held lingers (a
                // torso kept up) and a pistol taken out next is drawn.
                member.figure.drawing = crate::draw_anim::Drawing::at(false);
                None
            };
            let mut moved = None;
            if !still {
                let eye = self.eye.filter(|_| Some(id) == me);
                let look = looks
                    .iter()
                    .find(|look| look.0 == id)
                    .and_then(|&(_, other, arc)| {
                        Some((
                            self::soles(blended_for(history, other, ms, own)?.origin),
                            arc,
                        ))
                    });
                let posed = place(
                    member,
                    &at,
                    (soles, ms, eye),
                    (
                        posing.contains(&id) || falls || in_place || death_plays,
                        armed,
                        reach,
                    ),
                    (held, look, drawn),
                    self.holdings.wielded(id),
                    transforms,
                );
                moved = posed.as_ref().and_then(|posed| posed.root);
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
                && let Some(one) = as_drawn_dead(&self.rig, (id, member), at, moved, transforms)
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
        self.ragdolls.update(&dying, (level, history), ms);
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
        mine
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

/// What the conversations under way hide of `who`'s item models.
fn hidden(unequipped: &[Unequipped], who: SimId) -> Hide {
    unequipped
        .iter()
        .filter(|kept| kept.who == who)
        .map(|kept| kept.hide)
        .max()
        .unwrap_or(Hide::None)
}

/// `ANIMATION_LOOPING_HORROR`: the body's own `horror` clip.
const HORROR: u16 = 18;

/// When the hold on `fighter` began (sim ms), if it is held that way:
/// C11's hold, its round back from its end.
fn held_as(fighter: &Fighter, hold: Hold) -> Option<u64> {
    let force = &fighter.force;
    (force.hold == hold && fighter.alive()).then(|| {
        force
            .stunned_until
            .saturating_sub(sim::combat::force::ja::STUN_MS)
    })
}

/// The paralysis on `fighter` at `ms` (until when) and the moment its clips
/// stopped at: KOTOR's paralysed (Stasis) stand still, as their engine stops
/// their clips, the moment the hold was first drawn (`was`, the same hold as
/// drawn before), however long it lasts; none once it ends.
fn held_since(fighter: Option<&Fighter>, was: Option<(u64, f64)>, ms: f64) -> Option<(u64, f64)> {
    let until = fighter
        .filter(|f| f.force.hold == Hold::Paralyze && f.alive())
        .map(|f| f.force.stunned_until)
        .filter(|&until| ms < until as f64)?;
    Some(match was {
        Some((before, since)) if before == until => (until, since),
        _ => (until, ms),
    })
}

/// The clip a held `fighter` plays at `ms` while its hold lasts, as
/// KOTOR's engine plays its states: the horrified (Fear, Horror, Insanity)
/// clutch their heads in their own `horror` loop, a whirlwind's victim
/// spins in its `whirlwind`.
fn held_clip(fighter: &Fighter, ms: f64) -> Option<Acting> {
    let until = fighter.force.stunned_until;
    let (hold, animation) = match fighter.force.hold {
        Hold::Fear => (Hold::Fear, HORROR),
        Hold::Whirlwind => (Hold::Whirlwind, acting::WHIRLWIND),
        _ => return None,
    };
    let since = held_as(fighter, hold).filter(|_| ms < until as f64)?;
    Some(Acting {
        animation,
        since,
        ms: (until - since) as u32,
    })
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
    at: Blended,
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
    Some(Dying {
        id,
        pose: ragdolls::drawn_pose(&doll.rest, &member.body.nodes, transforms, lengths),
        doll,
        root: ragdoll::Frame::new(root.translation, root.rotation),
        at,
        died_ms: member.figure.died()? as f64,
        length_ms,
    })
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
fn edit(
    transforms: &mut Query<&mut Transform>,
    entity: Entity,
    change: impl FnOnce(&mut Transform),
) {
    if let Ok(mut transform) = transforms.get_mut(entity) {
        change(&mut transform);
    }
}

/// Puts a body's nodes back at `rest` before it is posed afresh: what
/// neither the frame's JA pose nor its clips key stands at rest, as KOTOR
/// leaves a node no animation keys, never as a clip since ended left it (a
/// lying clip's turn of the root dummy, which would tilt the whole body
/// once it stands again). Only a node that moved is written.
pub(crate) fn at_rest((nodes, rest): (&[Entity], &Rest), transforms: &mut Query<&mut Transform>) {
    for (&entity, &(_, translation, rotation)) in nodes.iter().zip(rest) {
        let moved = transforms
            .get(entity)
            .is_ok_and(|t| t.translation != translation || t.rotation != rotation);
        if moved {
            edit(transforms, entity, |t| {
                t.translation = translation;
                t.rotation = rotation;
            });
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

/// The gun `wielded` has out held up and aimed down its shots' way
/// (`asset_ja::rig::Aim`, KOTOR model space by `to_model` from the body at
/// `soles`): eased in over `AIM_EASE_MS` as the torso takes its ready or
/// shot animation with the gun out (`armed`) and its holder alive, and out
/// as the gun goes away (`HOLSTER_BLEND_MS`) or anything else plays; fully
/// aimed for `SHOT_AIM_MS` after a shot; kicked by its last shot
/// (`weapons::handling`); held up before the `eye` (`Cast::eye`) at `HOLD`.
/// None without a gun, or once eased out.
fn gun_aim(
    figure: &mut Figure,
    at: &Blended,
    (wielded, armed, drawn): (Option<&Wielded>, bool, Option<DrawPose>),
    ms: f64,
    (to_model, soles, eye): (Quat, Vec3, Option<Vec3>),
) -> Vec<Aim> {
    // What shoots nothing (a stun baton) is held up only before the eye.
    let guns = wielded.map_or([None, None], Wielded::held_ups);
    let guns = guns.map(|gun| gun.filter(|(blaster, _)| blaster.is_some() || eye.is_some()));
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
    // One gun's shots are the gun out's whichever hand the sim names.
    let shots = if pair {
        figure.shot_ms
    } else {
        [figure.shot_ms.into_iter().flatten().reduce(f64::max), None]
    };
    let shooting = shots
        .iter()
        .flatten()
        .any(|shot| (0.0..SHOT_AIM_MS).contains(&(ms - shot)));
    let mut ps = at.state;
    ps.view = at.view;
    ps.origin = at.origin;
    let [forward, right, up_axis] = view_axes(at);
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
        let Some((blaster, shape)) = gun else {
            continue;
        };
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
        // Up over `KICK_RISE_MS`, then settling.
        let kick = shots[hand].map_or(0.0, |shot| {
            let since = (ms - shot) as f32;
            if since < 0.0 {
                0.0
            } else if since < KICK_RISE_MS {
                since / KICK_RISE_MS
            } else {
                (-(since - KICK_RISE_MS) * 3.0 / handling.settle_ms).exp()
            }
        });
        // The barrel aims at the shot's way `CONVERGE` units out, where the
        // bolt drawn from the muzzle meets it (`combat_fx::Path`); what
        // shoots nothing, along the view. A pair's left gun from its own
        // muzzle (`sim::hand_muzzle`), held up mirrored before the eye.
        let side = if hand == 1 { Side::Left } else { Side::Right };
        let muzzle = Vec3::from_array(match blaster {
            Some(blaster) => sim::hand_muzzle(
                &ps,
                sim::combat::weapons::primary(blaster).muzzle,
                hand as u8,
            ),
            None => sim::muzzle(&ps, [0.0; 3]),
        });
        let target = muzzle + forward * crate::combat_fx::CONVERGE;
        let hold = eye.map(|eye| {
            let [ahead, aside, down] = HOLD;
            let aside = if side == Side::Left { -aside } else { aside };
            to_model * (eyes(at, eye) + forward * ahead + right * aside - up_axis * down - soles)
        });
        aims.push(Aim {
            side,
            target: to_model * (target * UNIT - soles),
            muzzle: shape.muzzle,
            barrel: shape.barrel,
            up: shape.up,
            support: handling
                .support
                .filter(|_| !pair)
                .map(|share| shape.foregrip(share)),
            forward: handling.forward,
            kick: (
                handling.kick_degrees.to_radians() * kick,
                handling.kick_back * kick,
            ),
            weight: *aim,
            hold,
            stow,
        });
    }
    aims
}

/// The share of the clips laid over a JA pose from which they cover it
/// whole.
const WHOLE: f32 = 0.999;

/// How fast a head turns toward whom its body looks at in a conversation,
/// and back (radians a second): KOTOR's look-at speed is 10, in a unit
/// not read [verify].
const HEAD_TURN_RAD_S: f32 = 10.0;

/// The node a body's head turns by (S7): the `headhook` its head model
/// hangs on, else its own head (`head_g`), else a creature model's `head`
/// (HK-47's, T3-M4's `Head`).
fn head_node(ir: &ModelIr) -> Option<usize> {
    [hooks::HEAD_HOOK, hooks::HEAD, "head"]
        .iter()
        .find_map(|name| {
            ir.nodes
                .iter()
                .position(|node| node.name.eq_ignore_ascii_case(name))
        })
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
}

impl Head {
    /// At `ms`, eases toward `target` (degrees from the body; none: ahead)
    /// at `HEAD_TURN_RAD_S` and turns node `node` (of `nodes`, its parents
    /// `rest`'s) so far about the model's up after the body's pose
    /// (`turn_in_model`). A node no pose moved since it was turned turns
    /// from where it stood before.
    fn turn(
        &mut self,
        (target, ms): (Option<f32>, f64),
        (nodes, rest): (&[Entity], &Rest),
        node: usize,
        transforms: &mut Query<&mut Transform>,
    ) {
        let seconds = self
            .ms
            .map_or(0.0, |was| ((ms - was) / 1000.0).clamp(0.0, 0.25)) as f32;
        self.ms = Some(ms);
        let most = HEAD_TURN_RAD_S.to_degrees() * seconds;
        self.yaw += (target.unwrap_or(0.0) - self.yaw).clamp(-most, most);
        if self.yaw.abs() < 0.01 && self.turned.is_none() {
            return;
        }
        let Some(&entity) = nodes.get(node) else {
            return;
        };
        let Ok(now) = transforms.get(entity).map(|t| t.rotation) else {
            return;
        };
        let from = match self.turned {
            Some((written, from)) if written == now => from,
            _ => now,
        };
        edit(transforms, entity, |t| t.rotation = from);
        if self.yaw.abs() < 0.01 {
            self.turned = None;
            return;
        }
        let turn = Quat::from_rotation_z(self.yaw.to_radians());
        turn_in_model(transforms, (nodes, rest), node, turn);
        self.turned = transforms.get(entity).ok().map(|t| (t.rotation, from));
    }
}

/// How far (degrees, to the left, within `arc` of it) a body standing at
/// `soles` (game space, metres) facing `facing` (degrees) turns its head to
/// look at the one standing at `other`.
fn look_toward(soles: Vec3, facing: f32, (other, arc): (Vec3, f32)) -> f32 {
    let to = other - soles;
    let aim = to.y.atan2(to.x).to_degrees();
    angles::wrap_180(aim - facing).clamp(-arc, arc)
}

/// Turns `node` by `turn` in the model's space, its ancestors (`rest`'s
/// parents, `nodes` their entities) posed as they stand.
fn turn_in_model(
    transforms: &mut Query<&mut Transform>,
    (nodes, rest): (&[Entity], &Rest),
    node: usize,
    turn: Quat,
) {
    let mut above = Quat::IDENTITY;
    let mut parent = rest.get(node).and_then(|r| r.0);
    while let Some(index) = parent {
        if let Some(t) = nodes.get(index).and_then(|&e| transforms.get(e).ok()) {
            above = t.rotation * above;
        }
        parent = rest.get(index).and_then(|r| r.0);
    }
    if let Some(&entity) = nodes.get(node) {
        edit(transforms, entity, |t| {
            t.rotation = (above.inverse() * turn * above * t.rotation).normalize();
        });
    }
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

/// The view's forward, right and up (game space).
fn view_axes(at: &Blended) -> [Vec3; 3] {
    let (forward, right, up) = collision::trig::angle_vectors(at.view);
    [forward, right, up].map(Vec3::from_array)
}

/// Puts a body where its character stands and, with `pose`, poses it, a
/// hand gripping a blade (while `armed`) reaching for it in `reach`
/// (`asset_ja::rig::Reach`) with the blades `wielded` holds, a gun out held
/// up and aimed (`gun_aim`). Looked through (`eye`, `Cast::eye`), its gun
/// and its blade at rest are held up before its eyes. In a conversation a
/// head looks at the one its line pairs it with (`look`: where that one
/// stands, and the head's arc, `Head`) after the pose. A script's animation
/// shows on every body, a droid's or a beast's as a humanoid's does, in a
/// conversation or out of one, its clip's events heard (`hear`). While one
/// holds the world (`world_held`) it plays out (`acting`), a droid's or a
/// beast's walk keeps to its stride, and a humanoid wholly on KOTOR's clips (nothing gripped or
/// aimed) is KOTOR's own clip on its model, its JA pose not made. With no
/// conversation under way, it is drawn as it was before scene mode (S7).
fn place(
    Member {
        body,
        figure,
        heard,
        ..
    }: &mut Member,
    at: &Blended,
    (soles, ms, eye): (Vec3, f64, Option<Vec3>),
    (pose, armed, reach): (bool, bool, &[(Blade, Hand)]),
    (world_held, look, drawn): (bool, Option<(Vec3, f32)>, Option<DrawPose>),
    wielded: Option<&Wielded>,
    transforms: &mut Query<&mut Transform>,
) -> Option<Posed> {
    let quarter = std::f32::consts::FRAC_PI_2;
    // A script's animation: moving, fighting or dying breaks it off. A
    // hold's plays whatever it does.
    let [x, y, _] = at.state.velocity;
    let held = at.fighter.as_ref().and_then(|f| held_clip(f, ms));
    // Guns in hand or on their way to or from the holsters (`draw_anim`)
    // keep the arms the pose's.
    let readying = drawn.is_some_and(|drawn| drawn.in_hand || drawn.stow > 0.0);
    let still =
        held.is_some() || (x * x + y * y).sqrt() < STILL_SPEED && at.alive && !armed && !readying;
    let acting = held.or(at.fighter.and_then(|f| f.acting));
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
            shows.follow(acting, (still, at.alive, world_held), ms);
            hear(heard, shows, (acted, figure.breath.wield), ms);
            throw.follow(at.fighter.as_ref());
            // A player's own body (its gait) keeps JA's; JA's own pain,
            // drawn on the pose, is the flinch where the sim plays it.
            let pained = gait.is_some() || pained(&at.state);
            flinch.follow(at.fighter.as_ref(), pained);
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
                    .then(|| crate::gait::of_legs(at.state.legs.anim))
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
            at_rest((&body.nodes, &rig.rest), transforms);
            let face = body.face.as_ref();
            if let Some(face) = face {
                face.stand((ms, at.alive), transforms);
            }
            // Each KOTOR clip over the body, and over its face.
            let lay = |shown: &mut dyn Iterator<Item = (&Animator, f32, f32)>,
                       transforms: &mut Query<&mut Transform>| {
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
                    let hilt =
                        eyes(at, eye) + forward * ahead + right * (aside * side(hand)) - up * down;
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
            let share = figure.reach_share(ms);
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
                let (mut most, mut free) = (asset_ja::rig::REACH, 0.0);
                if let Some(held) = &held {
                    let (hilt, rise) = held(hand);
                    let w = figure.held;
                    muzzle = muzzle.lerp(hilt, w);
                    direction = direction.normalize_or_zero().lerp(rise, w);
                    most += (BLADE_REACH - most) * w;
                    free = w;
                }
                reached.push(Reach {
                    grip: hand as usize,
                    muzzle,
                    direction,
                    origin: mean(&mut ends.iter().map(|end| end.0)),
                    along,
                    share,
                    most,
                    free,
                });
            }
            let gun = wielded.filter(|_| gripping.is_none());
            let aims = gun_aim(figure, at, (gun, armed, drawn), ms, (to_model, soles, eye));
            let head = |figure: &mut Figure, transforms: &mut Query<&mut Transform>| {
                if let Some(node) = body.head {
                    let target = look.map(|look| look_toward(soles, faces, look));
                    figure
                        .head
                        .turn((target, ms), (&body.nodes, &rig.rest), node, transforms);
                }
            };
            // While the world is held, a body wholly on KOTOR's clips (the
            // JA pose's share left under the clips laid over it in turn at
            // most `1 − WHOLE`), nothing gripped (no hand reaches, so
            // `reached` is empty) or aimed, is that clip on its model: its JA
            // pose is not made.
            let under: f32 = shown.iter().map(|&(_, _, weight)| 1.0 - weight).product();
            if world_held && under <= 1.0 - WHOLE && gripping.is_none() && aims.is_empty() {
                lay(
                    &mut shown.into_iter().chain(throw.shown(acted, ms)),
                    transforms,
                );
                head(figure, transforms);
                return None;
            }
            let ja = figure.pose_facing(ms, shown.is_empty());
            let posed = rig.retargeter.pose(
                &ja,
                Vec3::ZERO,
                (&reached, &aims),
                (&mut figure.off_hand, ms as f32),
            );
            for &(node, rotation) in &posed.rotations {
                if let Some(&entity) = body.nodes.get(node) {
                    edit(transforms, entity, |t| t.rotation = rotation);
                }
            }
            if let Some((node, offset)) = posed.root
                && let (Some(&entity), Some(&(_, rest, _))) =
                    (body.nodes.get(node), rig.rest.get(node))
            {
                edit(transforms, entity, |t| {
                    t.translation = rest + rig.animation_scale * offset;
                });
            }
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
            lay(
                &mut shown.into_iter().chain(throw.shown(acted, ms)),
                transforms,
            );
            head(figure, transforms);
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
            clips
                .shows
                .follow(acting, (still, at.alive, world_held), ms);
            clips.flinch.follow(at.fighter.as_ref(), false);
            // Its scripted clips show and sound as a retargeted body's do,
            // whether or not a conversation holds the world.
            hear(
                heard,
                &mut clips.shows,
                (&clips.acted, figure.breath.wield),
                ms,
            );
            // KOTOR models face +Y: yaw 0 (+X) is a quarter turn back.
            let faces = figure.yaw().unwrap_or(at.view[1]);
            edit(transforms, body.root, |t| {
                *t = Transform::from_translation(soles)
                    .with_rotation(Quat::from_rotation_z(faces.to_radians() - quarter));
            });
            if pose {
                at_rest((&body.nodes, &clips.rest), transforms);
                let playing = body.playing.as_mut();
                let face = body.face.as_ref();
                clips.pose(figure, at, (ms, world_held), transforms, playing, face);
                if let Some(node) = body.head {
                    let target = look.map(|look| look_toward(soles, faces, look));
                    let bound = (&body.nodes[..], &clips.rest[..]);
                    figure.head.turn((target, ms), bound, node, transforms);
                }
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
        (ms, held): (f64, bool),
        transforms: &mut Query<&mut Transform>,
        mut playing: Option<&mut Vec<(Arc<AnimationIr>, f32)>>,
        face: Option<&crate::face::Face>,
    ) {
        self.pose_own(figure, at, (ms, held), transforms);
        let own = self
            .shown
            .and_then(|(clip, at)| Some((self.clips[clip].as_ref()?, at)));
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
        let flinch = self
            .flinch
            .shown(&self.acted, ms, wield)
            .filter(|_| at.alive);
        for (clip, seconds, weight) in self
            .shows
            .shown(&self.acted, ms, (wield, None))
            .chain(flinch)
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
        (ms, held): (f64, bool),
        transforms: &mut Query<&mut Transform>,
    ) {
        let Some((clip, seconds)) = self.at(figure, at, (ms, held)) else {
            return;
        };
        if let Some((old, old_at)) = self.shown.filter(|&(old, _)| old != clip) {
            self.fading = Some((old, old_at, ms));
        }
        self.shown = Some((clip, seconds));
        let Some(animator) = &self.clips[clip] else {
            return;
        };
        let fade = self.fading.and_then(|(old, old_at, since)| {
            let toward = ((ms - since) as f32 / 1000.0) / animator.transition();
            let from = self.clips[old].as_ref()?;
            (toward < 1.0).then_some((from, old_at, toward.max(0.0)))
        });
        match fade {
            Some((from, from_at, toward)) => {
                animator.pose_from(seconds, (from, from_at), toward, transforms);
            }
            None => {
                self.fading = None;
                animator.pose(seconds, transforms);
            }
        }
    }

    /// How long its dying clip lasts (ms).
    fn die_ms(&self) -> Option<f64> {
        let die = self.clips[DIE].as_ref()?;
        Some(f64::from(die.length()) * 1000.0)
    }

    /// The clip shown and the one fading out, by name, for dumps.
    fn facts(&self) -> serde_json::Value {
        let name = |clip: Option<usize>| {
            clip.and_then(|clip| Some(self.clips[clip].as_ref()?.name().to_string()))
        };
        serde_json::json!({
            "shown": name(self.shown.map(|shown| shown.0)),
            "fading_from": name(self.fading.map(|fading| fading.0)),
        })
    }

    /// The clip a creature plays at `ms` and how far into it (seconds):
    /// dead, its dying clip from its death to the end, then its dead clip
    /// looped (or the dying clip held); alive, standing, walking or running,
    /// looped on the clock, but while the world is held (`held`, S7) walking
    /// and running by the distance covered, a cycle a stride
    /// (`gait::by_stride`), as a humanoid's gait does.
    fn at(
        &mut self,
        figure: &Figure,
        at: &Blended,
        (ms, held): (f64, bool),
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
        let length = |clip: usize| self.clips[clip].as_ref().map(Animator::length);
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
        let legs = at.state.legs.anim.name();
        let walking = legs.contains("WALK") || (speed < RUN_SPEED && !legs.contains("RUN"));
        let either = |first: usize, second: usize| {
            [first, second]
                .into_iter()
                .find(|&clip| self.clips[clip].is_some())
        };
        let clip = if speed < STILL_SPEED {
            either(PAUSE, PAUSE)
        } else if walking {
            either(WALK, RUN)
        } else {
            either(RUN, WALK)
        };
        match clip.or(either(PAUSE, PAUSE))? {
            clip @ (WALK | RUN) if held => Some((clip, self.cycles[clip - WALK])),
            clip => looped(clip, 0.0),
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
