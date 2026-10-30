//! `kjk play`'s view: the characters of the presented snapshots on their
//! KOTOR bodies, moving as JA animates players (`cast`), and JA's
//! third-person camera on the player of seat 0, from the presented
//! snapshots alone (`sim_host`).
//!
//! Each frame the view catches up with the presented time in steps of at
//! most a 60 Hz frame, following every tick's snapshot in order: what each
//! character's legs and torso start in a snapshot starts in its Ghoul2
//! player at that snapshot's time, and each step its look turns
//! (`asset_ja::angles`, as JA's game turns a player's look each client
//! frame) and the camera follows. The bodies are then placed between the
//! ticks around the moment and posed.
//!
//! The third-person camera keeps an aim point a little above the watched
//! player's eye and a place backed off behind it along the view, each
//! chasing where it should be by a share of the distance every 50 ms, the
//! place the more stiffly the steeper the view and the faster it turns; a
//! small box traced through the level and its doors keeps both out of the
//! walls (`follow_camera`). Without the quirks the camera does not lag
//! across the view and is not pulled into the player under a low ceiling.
//!
//! A joined player's own blades (I22 C20): each prediction followed sweeps
//! them from the one before, through the others as drawn when it shows
//! (`sim::seen_cuts`, the authority's sweep); each body met once a swing
//! is a contact `combat_fx` sparks at once.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use asset_ja::angles;
use asset_odyssey::anim::AnimationIr;
use asset_odyssey::hooks;
use asset_odyssey::model::ModelIr;
use asset_odyssey::module::Camera as PlacedCamera;
use bevy::prelude::*;
use collision::{Contents, Extents, UNIT};
use console::{CameraRequest, DumpDue, DumpFacts};
use sim::combat::damage::FriendlyFire;
use sim::module::ObjectKind;
use sim::motion;
use sim::{Instances, SimId, Snapshot};

use crate::armory::Rearms;
use crate::cast::{self, BodyRig, BodyRoots, Cast, Guns, Hitboxes, Own, Rig};
use crate::combat_fx::{Contact, Contacts};
use crate::first_person::{self, FirstPerson};
use crate::going_view::{Fading, Leaves};
use crate::lifecycle_view::SceneCamera;
use crate::made::Made;
use crate::scene::{LiveBlades, RetiredBlades, SceneContent, SceneModel, SceneRoot, game_to_bevy};
use crate::script_fx::OwnScreen;
use crate::shots::{self, OwnFire, Shot};
use crate::sim_blades::{self, SimBlades};
use crate::sim_host::{OwnSeat, OwnView, PresentedSnapshot, SceneSwap};
use crate::sound::Cues;
use crate::weapon_sounds::WeaponSounds;
use crate::weapons::{Owner, Props};

/// The longest step the view takes catching up (a 60 Hz frame).
const STEP_MS: f64 = 1000.0 / 60.0;
/// JA's `cg_fov` 80 across a 4:3 screen, as a vertical field of view.
const FOV_DEGREES: f32 = 64.94;
const CAMERA_BOX: f32 = 4.0;
const CAMERA_BACK_OFF: f32 = 80.0;
const AIM_ABOVE_EYE: f32 = sim::combat::mind::CROSSHAIR_ABOVE_EYE;
const AIM_DAMP: f32 = 0.5;
const PLACE_DAMP: f32 = 0.3;
const DAMP_INTERVAL_MS: f32 = 50.0;
/// How many frames the frame-time figures cover.
const FRAMES_KEPT: usize = 240;
/// How many of the own character's rewritten ticks a dump lists.
const REWRITES_KEPT: usize = 64;
/// The others a contact is looked for on: within two blades' reach and a
/// step's travel (units, the sim's `NEAR`).
const CONTACT_NEAR: f32 = 192.0;

/// Where the camera goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraMode {
    /// JA's third-person camera.
    Third,
    /// The player's own eyes (`first_person`); `Action::View` turns
    /// between it and `Third`.
    First,
    /// `distance` metres off the player along +X, 1.5 m above its soles,
    /// looking back and a little down at it (for the gait).
    Side(f32),
    /// The same, but 1.1 m above z 0 whatever the player does (for a
    /// jump's arc).
    Arc(f32),
}

/// What the view draws with: the JA skeleton and its sequences, the play
/// body and its retargeting, and the level to keep the camera out of.
pub struct PlayViewSetup {
    /// The first seat's body's index among the scene's models; each seat
    /// has one, `stride` models (body and head) after the last.
    pub body: usize,
    pub stride: usize,
    pub seats: u8,
    pub rig: Rig,
    /// Each seat's body lined up with JA's skeleton (the seats take them in
    /// turn); none draws JA's own model (its node `i + 1` is GLA bone `i`),
    /// for comparing with JA.
    pub body_rigs: Vec<Arc<BodyRig>>,
    /// Where to write each frame's node places (CSV), if anywhere.
    pub trace: Option<std::path::PathBuf>,
    /// The sim's level: its collision, traced with the presented doors.
    pub level: sim::Level,
    /// The camera asked for (`--camera`); none for the player's own choice:
    /// the own eyes when the settings keep them (`camera.view`), else JA's.
    pub camera: Option<CameraMode>,
    /// The players' weapons kept to their sheets.
    pub armory: crate::armory::Setup,
    /// What the creatures made in play are loaded with (`made`).
    pub made: crate::made::Setup,
    /// What the seats are dressed again with (`wardrobe`).
    pub wardrobe: crate::wardrobe::Setup,
    /// Draw the sim's blades beside the drawn ones (`sim_blades`).
    pub sim_blades: bool,
    /// The game's base items' wields (`stance::Wields`).
    pub wields: crate::stance::Wields,
}

/// Its systems; a session comes in with `install`.
pub struct PlayViewPlugin;

/// JA's third-person camera (units, degrees).
#[derive(Default)]
struct ThirdPerson {
    target: [f32; 3],
    location: [f32; 3],
    last_yaw: f32,
    started: bool,
}

/// Where the crosshair looks this frame (the third-person camera's ray,
/// metres), none while a conversation or a fixed camera has the view
/// (`target_view`).
#[derive(Resource, Default)]
pub struct Aim(pub Option<sim::target::Look>);

/// The set the view's drawing runs in, for what reads `Aim` after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Drawn;

/// This player's body model in the scene's models, for the screens' 3D
/// views (`gui_scene`).
#[derive(Resource, Default)]
pub struct OwnBody(pub Option<usize>);

fn own_body(
    view: Res<View>,
    own: Option<Res<crate::sim_host::OwnSeat>>,
    mut body: ResMut<OwnBody>,
) {
    let seat = own.map_or(sim::Seat(0), |own| own.0);
    let now = view.cast.body_of(seat);
    if body.0 != now {
        body.0 = now;
    }
}

/// The lit blades in hand drawn this frame, for the walls they meet
/// (`blade_fx`).
fn wielders(view: Res<View>, mut wielders: ResMut<crate::blade_fx::Wielders>) {
    wielders.0.clear();
    for (id, blades) in view.cast.blades_in_hand() {
        wielders
            .0
            .extend(blades.iter().map(|&(slot, spec)| (id, slot, spec)));
    }
}

#[derive(Resource)]
struct View {
    cast: Cast,
    level: sim::Level,
    camera_mode: CameraMode,
    /// Recent snapshots, oldest first, and the last one followed.
    history: Vec<Arc<Snapshot>>,
    followed: Option<u64>,
    render_ms: f64,
    started: bool,
    camera: ThirdPerson,
    first: FirstPerson,
    /// A `camera` command took the camera: it stays where it was put.
    fixed: bool,
    /// The trace file and the nodes it follows.
    trace: Option<(std::io::BufWriter<std::fs::File>, Vec<usize>)>,
    /// The doors' collision states in the latest snapshot.
    doors: Option<Vec<u8>>,
    /// The scene swap this view was built for.
    generation: u32,
    /// The scene's models by index: root and node entities.
    models: Vec<Option<(Entity, Vec<Entity>)>>,
    /// Seat 0's character, and the characters posed last frame.
    me: Option<SimId>,
    /// Whom the camera follows: `me`, else while the seat watches (I30 P3)
    /// the party member `follow` picks (`camera_subject`).
    watched: Option<SimId>,
    /// `watched` while the seat watches (its own body down or none), else
    /// none (`dialogue_view::Watching`).
    watching: Option<SimId>,
    follow: usize,
    close: Vec<SimId>,
    /// The last frames' times (ms).
    frames: VecDeque<f32>,
    /// A joined player's own predictions (none in solo or as the host).
    own: OwnTimeline,
    /// The keyframed sounds the figures passed since the last frame: each
    /// with whose, where (units) and when (the view's ms).
    keyed: Vec<(asset_ja::events::Keyed, (SimId, Vec3, f32))>,
    /// The dialog camera's shot as its line set it up (`talk_camera`), and
    /// the sides it chose for its conversation's pairs (`Sides::side`).
    dialog_shot: std::sync::Mutex<Option<DialogShot>>,
    sides: std::sync::Mutex<Sides>,
    /// Cutscene stunt clips taken out of their models, by model and clip.
    stunt_clips: std::sync::Mutex<StuntClips>,
    /// How far each participant's stunt clip was heard (`stage_stunts`).
    stunts_heard: StuntsHeard,
    /// The module's creatures gone from the world, their bodies hidden.
    departed: BTreeSet<u32>,
}

/// A joined player's own character, ahead of the host's snapshots: its
/// predictions, how far ahead of the view's time they are shown, the
/// correction, and the last one followed.
#[derive(Default)]
struct OwnTimeline {
    active: bool,
    history: Vec<Arc<Snapshot>>,
    shift: f64,
    correction: [f32; 3],
    followed: Option<u64>,
    /// The bodies the own swing has met so far, and the contacts found
    /// since the last frame.
    cut: Vec<SimId>,
    contacts: Vec<Contact>,
    /// The own shots and bolts found since the last frame.
    shots: Vec<Shot>,
    bolts: Vec<(u64, sim::combat::Missile)>,
    /// Predictions that changed what the own character's legs or torso
    /// play at a tick already shown (a rewind with the host's verdict):
    /// how many, and the latest (the own ms, what was shown, what is).
    rewrites: u64,
    rewritten: VecDeque<(u64, String, String)>,
}

impl OwnTimeline {
    /// The own character at the view's time `ms`, while predictions come.
    fn at(&self, me: Option<SimId>, ms: f64) -> Option<Own<'_>> {
        Some(Own {
            id: me.filter(|_| self.active)?,
            history: &self.history,
            ms: ms + self.shift,
            correction: self.correction,
        })
    }
}

/// C20: where the own blades (`me`), from `before` to `now` (its
/// predictions a tick apart), cut the others as drawn when `now` shows
/// (`history` at that ms); each body once a swing (`cut`, the bodies met
/// so far, cleared as a swing ends or another begins).
fn own_contacts(
    (level, doors): (&sim::Level, Option<&[u8]>),
    (history, ms): (&[Arc<Snapshot>], f64),
    me: SimId,
    (before, now): (&Snapshot, &Snapshot),
    cut: &mut Vec<SimId>,
) -> Vec<Contact> {
    let (Some(was), Some(is)) = (cast::entity(before, me), cast::entity(now, me)) else {
        return Vec::new();
    };
    let (Some(old), Some(f), Some(ps)) = (was.fighter, is.fighter, is.player) else {
        return Vec::new();
    };
    let same_swing = old.swing.saber_move == f.swing.saber_move && old.swing.flip == f.swing.flip;
    if !same_swing || f.trails.iter().all(Option::is_none) {
        cut.clear();
    }
    let near = |origin: [f32; 3]| {
        let d = sim::math::sub(origin, ps.origin);
        sim::math::dot(d, d) <= CONTACT_NEAR * CONTACT_NEAR
    };
    let bodies = bodies_as_drawn(level, (history, ms), is, near);
    if bodies.is_empty() {
        return Vec::new();
    }
    let sheet = cast::sheet(is, &Instances::in_snapshot(level, now));
    let cuts = sim::seen_cuts(level, doors, (&ps, &sheet), &old, &f, now.time_ms, &bodies);
    let mut contacts = Vec::new();
    for (victim, point, _) in cuts {
        if cut.contains(&victim) {
            continue;
        }
        cut.push(victim);
        contacts.push(Contact {
            by: me,
            victim,
            point: Vec3::from_array(point),
        });
    }
    contacts
}

/// The bodies of the others that `me` (as `is`) may strike, as drawn at
/// `ms` in `history`, those `near` only: with friendly fire off, a party
/// member's blades and shots pass through the party's (`Targets::met_by`).
fn bodies_as_drawn(
    level: &sim::Level,
    (history, ms): (&[Arc<Snapshot>], f64),
    is: &sim::EntityState,
    near: impl Fn([f32; 3]) -> bool,
) -> Vec<collision::Body> {
    let Some(shown) = history.last() else {
        return Vec::new();
    };
    let spares = level.friendly_fire() == FriendlyFire::Off && shown.in_party(is);
    shown
        .entities
        .iter()
        .filter(|e| e.id != is.id && e.fighter.is_some())
        .filter(|e| !(spares && shown.in_party(e)))
        .filter_map(|e| cast::blended(history, e.id, ms).map(|at| (e.id, at)))
        .filter(|(_, at)| at.alive && near(at.origin))
        .map(|(id, at)| {
            let mut stands = at.state;
            stands.origin = at.origin;
            sim::body(id, &stands)
        })
        .collect()
}

/// The nodes a trace follows.
const TRACED: [&str; 11] = [
    "pelvis_g",
    "head_g",
    "lhand_g",
    "rhand_g",
    "lfoot_g",
    "rfoot_g",
    "torsoupr_g",
    "lbicep_g",
    "rbicep_g",
    "lforearm_g",
    "rforearm_g",
];

impl Plugin for PlayViewPlugin {
    fn build(&self, app: &mut App) {
        // Empty until a session is installed; the sounds read them meanwhile.
        app.init_resource::<Guns>()
            .init_resource::<BodyRoots>()
            .init_resource::<crate::made::MineTints>()
            .init_resource::<crate::stealth_view::Shells>()
            .init_resource::<crate::going_view::Fading>()
            .init_resource::<Cues>()
            .init_resource::<crate::severing::Cuts>()
            .init_resource::<crate::severing::Smoulders>()
            .init_resource::<OwnBody>()
            .init_resource::<Aim>()
            .init_resource::<first_person::Eyes>()
            .add_systems(Update, own_body.run_if(resource_exists::<View>))
            .add_systems(
                PostUpdate,
                wielders
                    .run_if(resource_exists::<View>)
                    .after(Drawn)
                    .before(crate::scene::BladesDrawn),
            )
            .add_systems(
                PostUpdate,
                // Made creatures spawned before the view looks for them.
                (
                    crate::severing::uncut,
                    crate::armory::arm,
                    crate::made::track,
                    crate::wardrobe::dress,
                    follow_keys,
                    view_keys,
                    draw.in_set(Drawn),
                    crate::severing::apply,
                )
                    .chain()
                    .run_if(resource_exists::<View>)
                    .before(bevy::transform::TransformSystems::Propagate)
                    // A cut's pieces drawn the frame its limb first flies:
                    // their meshes known to the renderer this frame (the
                    // whole mesh, stretched, shown meanwhile).
                    .before(bevy::asset::AssetEventSystems),
            )
            .add_systems(
                PostUpdate,
                // A mine's dome tinted, a stealth shell and a body going
                // faded once their meshes stand.
                (
                    crate::made::tint_mines,
                    crate::stealth_view::shells,
                    crate::going_view::fade,
                )
                    .run_if(resource_exists::<View>)
                    .after(Drawn),
            )
            .add_systems(Update, crate::severing::cool)
            // A sprung mine's blast, heard before the sounds are taken.
            .add_systems(
                Update,
                crate::made::sprung
                    .run_if(resource_exists::<View>)
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .run_if(resource_exists::<crate::ambience::Heard>),
            )
            .add_systems(
                PostUpdate,
                crate::severing::smoulder
                    .run_if(resource_exists::<crate::sim_host::PresentedSnapshot>)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(crate::combat_fx::FightDrawn),
            )
            .add_systems(
                PostUpdate,
                crate::sim_blades::show
                    .run_if(resource_exists::<SimBlades>)
                    .after(crate::scene::BladesDrawn)
                    .before(bevy::asset::AssetEventSystems),
            );
    }
}

/// The view of a session, put into `app`.
pub fn install(app: &mut World, setup: PlayViewSetup) {
    {
        let trace = setup.trace.as_ref().and_then(|path| {
            use std::io::Write as _;
            let names = setup.body_rigs.first()?.node_names();
            let found: Vec<(&str, usize)> = TRACED
                .iter()
                .filter_map(|t| Some((*t, names.iter().position(|n| n.eq_ignore_ascii_case(t))?)))
                .collect();
            let nodes: Vec<usize> = found.iter().map(|f| f.1).collect();
            let mut file = std::io::BufWriter::new(std::fs::File::create(path).ok()?);
            let header: Vec<String> = found
                .iter()
                .map(|(t, _)| *t)
                .chain(asset_ja::rig::JA_ARMS)
                .enumerate()
                .flat_map(|(i, t)| {
                    let ja = if i < found.len() { "" } else { "ja_" };
                    ["x", "y", "z"].map(|a| format!("{ja}{t}_{a}"))
                })
                .collect();
            writeln!(file, "ms,legs,torso,{},correction", header.join(",")).ok()?;
            Some((file, nodes))
        });
        app.init_resource::<Hitboxes>();
        // In a window the player's choice stands; headless runs take JA's
        // camera unless a settings file is named for them (`KJK_SETTINGS`).
        let windowed = app
            .get_resource::<crate::scene::Headless>()
            .is_some_and(|headless| !headless.0)
            || std::env::var_os("KJK_SETTINGS").is_some();
        let chose_first = app
            .get_resource::<crate::settings::Settings>()
            .is_some_and(crate::settings::Settings::first_person);
        let camera_mode = setup.camera.unwrap_or(if windowed && chose_first {
            CameraMode::First
        } else {
            CameraMode::Third
        });
        app.insert_resource(View {
            cast: Cast::new(
                setup.rig,
                cast::Bodies {
                    first: setup.body,
                    stride: setup.stride,
                    seats: setup.seats,
                    dressed: BTreeMap::new(),
                },
                setup.body_rigs,
                setup.wields,
            ),
            level: setup.level,
            camera_mode,
            history: Vec::new(),
            followed: None,
            render_ms: 0.0,
            started: false,
            camera: ThirdPerson::default(),
            first: FirstPerson::default(),
            fixed: false,
            trace,
            doors: None,
            generation: 0,
            models: Vec::new(),
            me: None,
            watched: None,
            watching: None,
            follow: 0,
            close: Vec::new(),
            frames: VecDeque::with_capacity(FRAMES_KEPT),
            own: OwnTimeline::default(),
            keyed: Vec::new(),
            stunt_clips: std::sync::Mutex::default(),
            stunts_heard: StuntsHeard::default(),
            departed: BTreeSet::new(),
            dialog_shot: std::sync::Mutex::default(),
            sides: std::sync::Mutex::default(),
        });
        crate::minigame_view::install(app, setup.made.installer.clone());
        crate::script_fx::install(app, setup.made.installer.clone());
        crate::blaster_view::install(app, setup.made.installer.clone());
        crate::video_effect::install(app, setup.made.installer.clone());
        crate::grenade_view::install(app, setup.made.installer.clone());
        app.insert_resource(crate::armory::Armory::new(setup.armory));
        app.insert_resource(crate::made::Made::new(setup.made));
        app.insert_resource(crate::wardrobe::Wardrobe::new(setup.wardrobe));
        app.init_resource::<Rearms>();
        if setup.sim_blades {
            app.init_resource::<SimBlades>();
        }
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Where the camera's 4-unit box from `from` to `to` (units) stops in the
/// level with its doors, if short.
fn stop_short(
    (level, doors): (&sim::Level, Option<&[u8]>),
    from: [f32; 3],
    to: [f32; 3],
) -> [f32; 3] {
    let metres = |v: [f32; 3]| v.map(|x| x * UNIT);
    let extents = Extents {
        mins: [-CAMERA_BOX * UNIT; 3],
        maxs: [CAMERA_BOX * UNIT; 3],
    };
    let hit = level.with(doors, |world| {
        world.trace(
            metres(from),
            metres(to),
            &extents,
            Contents::SOLID | Contents::PLAYER_CLIP,
        )
    });
    if hit.fraction < 1.0 {
        lerp(from, to, hit.fraction)
    } else {
        to
    }
}

/// `d` with its sideways offset from `ideal` taken out: whatever lagged
/// across the view (along `right`) is put back on the line through the
/// ideal point.
fn onto_line(d: [f32; 3], ideal: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    let off: f32 = (0..3).map(|k| (d[k] - ideal[k]) * right[k]).sum();
    std::array::from_fn(|k| d[k] - off * right[k])
}

/// `ideal` with `keep` of the way back to `old` left: a damped chase.
fn chase(ideal: [f32; 3], old: [f32; 3], keep: f32) -> [f32; 3] {
    std::array::from_fn(|k| ideal[k] - keep * (ideal[k] - old[k]))
}

/// One frame of JA's third-person camera on the player `at`, `frame_ms`
/// after the last. The camera keeps an aim point a little above the eye
/// and a place backed off behind it along the view; each chases where it
/// should be, closing a share of the distance every 50 ms, the place the
/// more stiffly the steeper the view and the faster it turns. `trace`
/// stops a move of the camera's box short of the level. With `quirks` the
/// camera keeps JA's own faults: it backs off from the aim point as if no
/// ceiling were in the way (Q-0016), traces straight to its place, and
/// lags sideways as well.
fn follow_camera(
    camera: &mut ThirdPerson,
    at: &cast::Blended,
    quirks: bool,
    frame_ms: f32,
    trace: &dyn Fn([f32; 3], [f32; 3]) -> [f32; 3],
) {
    let pitch = at.view[0].clamp(-80.0, 80.0);
    let yaw = at.view[1];
    let (ahead, _, _) = sim::math::angle_vectors([pitch, yaw, at.view[2]]);
    let [x, y, z] = at.origin;
    let focus = [x, y, z + at.state.eye_height];
    let aim = [focus[0], focus[1], focus[2] + AIM_ABOVE_EYE];
    let backed_from = if quirks { aim } else { trace(focus, aim) };
    let place: [f32; 3] = std::array::from_fn(|i| backed_from[i] - CAMERA_BACK_OFF * ahead[i]);
    // From an aim point out to a place: straight, as JA traces it; or
    // level first and then up or down, so a ceiling stops the rise but not
    // the way back.
    let put = |target: [f32; 3], location: [f32; 3]| {
        if quirks {
            trace(target, location)
        } else {
            let level = trace(target, [location[0], location[1], target[2]]);
            trace(level, [level[0], level[1], location[2]])
        }
    };

    if !camera.started || frame_ms <= 0.0 {
        camera.target = trace(focus, aim);
        camera.location = put(camera.target, place);
        camera.last_yaw = yaw;
        camera.started = true;
        return;
    }

    // A fast turn stiffens the place's chase.
    let mut turned = (yaw - camera.last_yaw).abs();
    if turned > 180.0 {
        turned = (turned - 360.0).abs();
    }
    let rate = turned / frame_ms;
    let stiffness = if rate < 1.0 {
        0.0
    } else if rate > 2.5 {
        0.75
    } else {
        (rate - 1.0) * 0.5
    };
    camera.last_yaw = yaw;

    let intervals = frame_ms / DAMP_INTERVAL_MS;
    let (_, right, _) = sim::math::angle_vectors([0.0, yaw, 0.0]);
    let straighten = |d: [f32; 3], ideal: [f32; 3]| {
        if quirks {
            d
        } else {
            onto_line(d, ideal, right)
        }
    };

    let keep = (1.0 - AIM_DAMP).powf(intervals);
    let target = straighten(chase(aim, camera.target, keep), aim);
    camera.target = trace(focus, target);

    let p = pitch.abs() / 115.0;
    let mut damp = (1.0 - PLACE_DAMP) * p * p + PLACE_DAMP;
    if stiffness > 0.0 {
        damp += (1.0 - damp) * stiffness;
    }
    let location = if damp >= 1.0 {
        place
    } else {
        chase(place, camera.location, (1.0 - damp).powf(intervals))
    };
    camera.location = put(camera.target, straighten(location, place));
}

impl View {
    /// Follows the snapshots up to `ms` not followed yet: the own
    /// character in its predictions, when there are some.
    fn follow(&mut self, ms: f64) {
        let own = self.me.filter(|_| self.own.active);
        let followed = self.followed;
        for snapshot in self
            .history
            .iter()
            .filter(|s| followed.is_none_or(|f| s.tick > f) && s.time_ms as f64 <= ms)
        {
            self.followed = Some(snapshot.tick);
            self.cast.follow((snapshot, &self.level), own, None);
        }
        if own.is_none() {
            return;
        }
        let followed = self.own.followed;
        let until = ms + self.own.shift;
        let own_history = &self.own.history;
        for (at, snapshot) in own_history.iter().enumerate() {
            if followed.is_some_and(|f| snapshot.tick <= f) || snapshot.time_ms as f64 > until {
                continue;
            }
            self.own.followed = Some(snapshot.tick);
            self.cast.follow((snapshot, &self.level), None, own);
            if let (Some(me), Some(before)) = (own, at.checked_sub(1)) {
                let correction = Vec3::from_array(self.own.correction);
                let fired = shots::shots(&own_history[before], snapshot, &self.level, None);
                self.own
                    .shots
                    .extend(
                        fired
                            .into_iter()
                            .filter(|shot| shot.by == me)
                            .map(|shot| Shot {
                                muzzle: shot.muzzle + correction,
                                ..shot
                            }),
                    );
                self.own.bolts.extend(
                    shots::fired_bolts(snapshot, me)
                        .into_iter()
                        .map(|bolt| (snapshot.tick, bolt)),
                );
                let shown = snapshot.time_ms as f64 - self.own.shift;
                let others = (&self.history[..], shown);
                let contacts = own_contacts(
                    (&self.level, self.doors.as_deref()),
                    others,
                    me,
                    (&own_history[before], snapshot),
                    &mut self.own.cut,
                );
                let correction = Vec3::from_array(self.own.correction);
                self.own
                    .contacts
                    .extend(contacts.into_iter().map(|mut contact| {
                        contact.point += correction;
                        contact
                    }));
            }
        }
    }

    /// Moves the third-person camera along with the watched player as the
    /// view shows them (`follow_camera`), kept out of the level and its
    /// doors by the camera's box. Nothing happens with no one to watch.
    fn third_person(&mut self, ms: f64, frame_ms: f32) {
        let own = self.own.at(self.me, ms);
        let Some(at) = self.subject(ms, own.as_ref()) else {
            return;
        };
        let walls = (&self.level, self.doors.as_deref());
        let trace = |from, to| stop_short(walls, from, to);
        follow_camera(
            &mut self.camera,
            &at,
            self.cast.rig.quirks,
            frame_ms,
            &trace,
        );
    }

    /// Whom the camera follows at `ms`: the seat's remote droid while it
    /// flies one (no party member to drive, the world not held), else the
    /// watched body.
    fn subject(&self, ms: f64, own: Option<&cast::Own<'_>>) -> Option<cast::Blended> {
        cast::remote_at(&self.history, self.cast.seat, ms, 0.0).or_else(|| {
            self.watched
                .and_then(|id| cast::blended_for(&self.history, id, ms, own))
        })
    }

    /// One step of the view to `ms`, `frame_ms` after the last.
    fn advance(&mut self, ms: f64, frame_ms: f32) {
        self.follow(ms);
        let own = self.own.at(self.me, ms);
        self.cast
            .steer(&self.history, ms, frame_ms, &self.close, own.as_ref());
        for (id, keyed) in self.cast.keyframed(ms) {
            if let Some(at) = cast::blended(&self.history, id, ms) {
                let at = Vec3::from_array(at.origin);
                self.keyed.push((keyed, (id, at, ms as f32)));
            }
        }
        if !self.fixed {
            self.third_person(ms, frame_ms);
        }
    }

    /// Starts over in a new world, in a new scene or the same one: the
    /// characters, the snapshots, the camera. Blades' indices returned to
    /// retire.
    fn reset(&mut self, swap: &SceneSwap, commands: &mut Commands) -> Vec<usize> {
        self.generation = swap.generation;
        let retired = self.cast.reset(swap.body, !swap.scene, commands);
        self.level = swap.level.clone();
        self.history.clear();
        self.followed = None;
        self.started = false;
        self.camera = ThirdPerson::default();
        self.first.restart();
        self.me = None;
        self.watched = None;
        self.watching = None;
        self.close.clear();
        self.own = OwnTimeline::default();
        self.departed.clear();
        retired
    }

    /// Takes a joined player's new predictions (a rewind's replace the
    /// ticks they repeat) and how far ahead of the host's they are shown.
    fn take_own(&mut self, own: &OwnView, target: f64) {
        self.own.active = true;
        let parts = |snapshot: &Snapshot, me: SimId| {
            let p = cast::entity(snapshot, me)?.player?;
            Some((p.legs.anim, p.legs.flip, p.torso.anim, p.torso.flip))
        };
        let name = |(legs, _, torso, _): (motion::Anim, bool, motion::Anim, bool)| {
            format!("{} / {}", legs.name(), torso.name())
        };
        // What the ticks already shown played, before the new predictions
        // replace them.
        let followed = self.own.followed.unwrap_or(0);
        let shown: BTreeMap<u64, _> = self
            .me
            .map(|me| {
                self.own
                    .history
                    .iter()
                    .filter(|held| held.tick <= followed)
                    .filter_map(|held| Some((held.tick, parts(held, me)?)))
                    .collect()
            })
            .unwrap_or_else(BTreeMap::new);
        for snapshot in &own.fresh {
            if let (Some(was), Some(is)) = (
                shown.get(&snapshot.tick),
                self.me.and_then(|me| parts(snapshot, me)),
            ) && *was != is
            {
                self.own.rewrites += 1;
                self.own
                    .rewritten
                    .push_back((snapshot.time_ms, name(*was), name(is)));
                if self.own.rewritten.len() > REWRITES_KEPT {
                    self.own.rewritten.pop_front();
                }
            }
            let keep = self
                .own
                .history
                .partition_point(|held| held.tick < snapshot.tick);
            self.own.history.truncate(keep);
            self.own.history.push(Arc::new(snapshot.clone()));
        }
        let own_target = crate::sim_host::view_ms(&own.presented);
        self.own.shift = own_target - target;
        self.own.correction = own.correction;
        let until = self.render_ms + self.own.shift - 100.0;
        let old = self
            .own
            .history
            .iter()
            .take_while(|s| (s.time_ms as f64) < until)
            .count();
        // The last one before the moment stays: it is blended from.
        self.own.history.drain(..old.saturating_sub(1));
    }
}

/// Whom the camera follows: the seat's standing body (`me`), else
/// (watching: no body, I30 P3, or down, P11) the
/// `follow`th living party member in id order, round: the host's first.
/// The seat watches whenever this is not `me`.
pub(crate) fn camera_subject(
    snapshot: &Snapshot,
    me: Option<SimId>,
    follow: usize,
) -> Option<SimId> {
    me.or_else(|| {
        let party: Vec<SimId> = snapshot
            .entities
            .iter()
            .filter(|entity| snapshot.in_party(entity) && entity.fighter.is_some_and(|f| f.alive()))
            .map(|entity| entity.id)
            .collect();
        (!party.is_empty()).then(|| party[follow % party.len()])
    })
}

/// A creature's body let go: a module's creature's model hidden, a made
/// one's despawned, its character forgotten with its hitbox and weapons
/// (their blades' indices returned to retire).
fn let_go(
    cast: &mut Cast,
    models: &[Option<(Entity, Vec<Entity>)>],
    leaves: Leaves,
    hitboxes: &mut Hitboxes,
    visibility: &mut Query<&mut Visibility>,
    commands: &mut Commands,
) -> Vec<usize> {
    match leaves {
        Leaves::Placed(body, object) => {
            if let Some(Some((root, _))) = models.get(body)
                && let Ok(mut shown) = visibility.get_mut(*root)
            {
                *shown = Visibility::Hidden;
            }
            let Some((id, blades)) = cast.forget_body((body, object), commands) else {
                return Vec::new();
            };
            hitboxes.0.remove(&id);
            blades
        }
        Leaves::Made(gone) => {
            hitboxes.0.remove(&gone.id);
            commands.entity(gone.root).despawn();
            cast.forget((gone.id, Owner::Creature(gone.index)), commands)
        }
    }
}

/// A watching seat's key (`Action::Follow`, N unless rebound) follows the
/// next party member.
fn follow_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<crate::settings::KeyMap>>,
    screens: Option<Res<crate::ui_view::UiOpen>>,
    mut view: ResMut<View>,
) {
    let (Some(keys), Some(map)) = (keys, map) else {
        return;
    };
    if view.watched == view.me || screens.is_some_and(|open| open.0) {
        return;
    }
    if map.pressed_now(&keys, crate::settings::Action::Follow) {
        view.follow = view.follow.wrapping_add(1);
    }
}

/// The view key (`Action::View`, F1 unless rebound) turns between the own
/// eyes and JA's camera; the settings keep the choice.
fn view_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<crate::settings::KeyMap>>,
    screens: Option<Res<crate::ui_view::UiOpen>>,
    settings: Option<ResMut<crate::settings::Settings>>,
    mut view: ResMut<View>,
) {
    let (Some(keys), Some(map)) = (keys, map) else {
        return;
    };
    if screens.is_some_and(|open| open.0) || !map.pressed_now(&keys, crate::settings::Action::View)
    {
        return;
    }
    view.camera_mode = match view.camera_mode {
        CameraMode::Third => CameraMode::First,
        CameraMode::First => CameraMode::Third,
        other => other,
    };
    if let Some(mut settings) = settings {
        settings.keep_first_person(view.camera_mode == CameraMode::First);
    }
}

/// What the view takes over each frame: seats armed again, blades retired,
/// the own shots, creatures made in play, seats dressed again, the dead
/// left lying.
type Handovers<'w> = (
    ResMut<'w, Rearms>,
    ResMut<'w, RetiredBlades>,
    Option<ResMut<'w, OwnFire>>,
    ResMut<'w, Made>,
    ResMut<'w, crate::wardrobe::Wardrobe>,
    ResMut<'w, Fading>,
);

/// Who hangs on whom: each entity's parent and children.
type Family<'w, 's> = (
    Query<'w, 's, &'static ChildOf>,
    Query<'w, 's, &'static Children>,
);

/// Real time, a script's screen up, and KOTOR's weapon sounds.
type Pace<'w> = (
    Res<'w, Time<Real>>,
    Option<Res<'w, OwnScreen>>,
    Option<Res<'w, WeaponSounds>>,
);

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw(
    presented: Res<PresentedSnapshot>,
    (seat, own, mut contacts): (Res<OwnSeat>, Option<Res<OwnView>>, Option<ResMut<Contacts>>),
    (mut rearms, mut retired, mut fire, mut made, mut wardrobe, mut fading): Handovers,
    swap: Res<SceneSwap>,
    request: Res<CameraRequest>,
    scene: Res<SceneContent>,
    (time, screen, kotor): Pace,
    mut view: ResMut<View>,
    (mut hitboxes, mut guns, mut roots, mut eyes): (
        ResMut<Hitboxes>,
        ResMut<Guns>,
        ResMut<BodyRoots>,
        ResMut<first_person::Eyes>,
    ),
    scene_models: Query<(Entity, &SceneModel)>,
    root: Query<Entity, With<SceneRoot>>,
    (mut transforms, mut visibility, globals, (parents, children)): (
        Query<&mut Transform>,
        Query<&mut Visibility>,
        Query<&GlobalTransform>,
        Family,
    ),
    (mut lives, mut sim_blades, (mut aim, mut camera_aim), watching): (
        ResMut<LiveBlades>,
        Option<ResMut<SimBlades>>,
        (ResMut<Aim>, ResMut<crate::sim_host::CameraAim>),
        Option<ResMut<crate::dialogue_view::Watching>>,
    ),
    mut cameras: Query<(Entity, &mut Projection), SceneCamera>,
    (mut facts, dump_due, mut cues, mut cuts, mut hooks, mut body_clips, mut steps): (
        ResMut<DumpFacts>,
        Res<DumpDue>,
        ResMut<Cues>,
        ResMut<crate::severing::Cuts>,
        Option<ResMut<crate::script_fx::HookVisuals>>,
        Option<ResMut<crate::own_fx::BodyClips>>,
        ResMut<crate::ambience::ClipSteps>,
    ),
    mut commands: Commands,
) {
    let view = &mut *view;
    // No ray until the camera is placed this frame.
    aim.0 = None;
    if request.is_changed() && !request.is_added() {
        view.fixed = true;
    }
    // A new world (a module installed, a joined player starting in the
    // host's): the view starts over from the snapshot it arrived in.
    if swap.generation != view.generation {
        retired.0.extend(view.reset(&swap, &mut commands));
        hitboxes.0.clear();
    }
    fading.starts(swap.generation);
    let now_ms = presented.0.current().time_ms;
    let objects = sim::Instances::in_snapshot(&view.level, presented.0.current());
    // A body a dead creature leaves lies on (`going_view`); the rest go now.
    let lies = |view: &View, id: SimId, index: u32| {
        view.cast
            .figure(id)
            .is_some_and(|figure| figure.died().is_some())
            && crate::going_view::lies_on(&view.level, objects.get(index))
    };
    // A new scene, or made creatures' models added to it (`made`).
    if scene.is_changed() || view.models.len() != scene.0.models.len() {
        view.models = cast::model_index(&scene_models, scene.0.models.len());
    }
    view.cast.seed(&scene.0, &view.models);
    // Made creatures come hold their weapons once drawn; those gone go,
    // with them.
    for arrival in made.arrived.drain(..) {
        view.cast.recast(arrival.id);
        let at = (Owner::Creature(arrival.index), None);
        retired.0.extend(
            view.cast
                .rearm(at, &arrival.sheet, arrival.arms, &mut commands),
        );
    }
    let mut leaving: Vec<Leaves> = Vec::new();
    for gone in made.left.drain(..) {
        if lies(view, gone.id, gone.index) {
            hitboxes.0.remove(&gone.id);
            fading.lay((gone.id, gone.root), Leaves::Made(gone), now_ms);
        } else {
            leaving.push(Leaves::Made(gone));
        }
    }
    view.doors = view.level.states_in(presented.0.current());
    for snapshot in &presented.1 {
        if view.history.last().is_none_or(|s| s.tick < snapshot.tick) {
            if let Some(sim) = sim_blades.as_deref_mut() {
                sim.check(&view.level, snapshot);
            }
            view.history.push(snapshot.clone());
        }
    }
    let Some(newest) = view.history.last() else {
        return;
    };
    view.cast.seat = seat.0;
    view.me = cast::me(newest, seat.0);
    // A downed body is watched from the party like a seat with none (I30
    // P11).
    let standing = newest.standing(seat.0).map(|entity| entity.id);
    view.watched = camera_subject(newest, standing, view.follow);
    view.watching = view.watched.filter(|_| standing.is_none());
    if let Some(mut watching) = watching.filter(|watching| watching.0 != view.watching) {
        watching.0 = view.watching;
    }
    // Characters the armory armed again; any from a scene gone are gone
    // with it.
    for rearm in rearms.0.drain(..) {
        if rearm.generation != swap.generation {
            continue;
        }
        let at = rearm.holder();
        let id = at.1;
        retired
            .0
            .extend(view.cast.rearm(at, &rearm.sheet, rearm.arms, &mut commands));
        if let Some(id) = id.filter(|_| rearm.draws) {
            view.cast.draws(id);
        }
    }
    // Seats dressed again, once their new bodies are spawned; any from a
    // scene gone are gone with it.
    let models = &view.models;
    let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut wardrobe.done)
        .into_iter()
        .filter(|redress| redress.generation == swap.generation)
        .partition(|redress| models.get(redress.body).is_some_and(Option::is_some));
    wardrobe.done = waiting;
    for redress in ready {
        let seat = sim::Seat(redress.seat);
        if let Some((old, _)) = view
            .cast
            .body_of(seat)
            .and_then(|old| view.models.get(old)?.clone())
            && let Ok(mut shown) = visibility.get_mut(old)
        {
            *shown = Visibility::Hidden;
        }
        let ir = &scene.0.models[redress.body].ir;
        let rig = cast::BodyRig::new(ir, &view.cast.rig, redress.hold)
            .walking(redress.gait)
            .with_acted(redress.acted);
        let rig = Arc::new(rig);
        let id = newest.own(seat).map(|entity| entity.id);
        retired.0.extend(
            view.cast
                .redress((seat, id), redress.body, rig, &mut commands),
        );
    }
    view.cast.cast(
        newest,
        &view.level,
        &scene.0,
        &view.models,
        &mut hitboxes,
        &mut commands,
    );
    // Each in the body it wears: a disguise's once `made` has it.
    view.cast.disguise(
        (newest, &view.level),
        |id| made.disguise(id),
        &scene.0,
        &view.models,
        (&mut hitboxes, &mut visibility),
        &mut commands,
    );
    // A creature the module placed is drawn while the snapshot has it, in
    // the world or departing: one destroyed (a companion made again from
    // its template, a script's `DestroyObject`) leaves with its weapons, as
    // one made in play does (`made`'s own). Absent from both the newest
    // drawn and the newest presented, so a guest's rebuilt history lagging
    // its remote one does not take a creature away for good.
    let present: BTreeSet<u32> = [newest, presented.0.current()]
        .into_iter()
        .flat_map(|snapshot| snapshot.all_drawn())
        .filter_map(|entity| match entity.kind {
            sim::Kind::Object(ObjectKind::Creature, index) => Some(index),
            _ => None,
        })
        .collect();
    for creature in &scene.0.creature_views {
        if present.contains(&creature.object)
            || made.makes(creature.object)
            || !view.departed.insert(creature.object)
        {
            continue;
        }
        let leaves = Leaves::Placed(creature.body, creature.object);
        let fallen = view
            .cast
            .fallen_in(creature.body)
            .filter(|id| lies(view, *id, creature.object));
        match (fallen, view.models.get(creature.body)) {
            (Some(id), Some(Some((root, _)))) => {
                hitboxes.0.remove(&id);
                fading.lay((id, *root), leaves, now_ms);
            }
            _ => leaving.push(leaves),
        }
    }
    leaving.extend(fading.done(now_ms).into_iter().map(|lying| lying.leaves));
    for leaves in leaving {
        let (cast, models) = (&mut view.cast, view.models.as_slice());
        let blades = let_go(
            cast,
            models,
            leaves,
            &mut hitboxes,
            &mut visibility,
            &mut commands,
        );
        retired.0.extend(blades);
    }
    let seat_bodies = view.cast.show_seats(newest, &view.models, &mut visibility);
    guns.0 = view.cast.guns();
    roots.0 = view.cast.roots();
    let target = crate::sim_host::view_ms(&presented.0);
    if let Some(own) = &own {
        view.take_own(own, target);
    }
    if !view.started {
        // From the first snapshot on, however far the sim has gone: every
        // run reaches a moment the same way.
        let Some(first) = view.history.first().map(|s| s.time_ms as f64) else {
            return;
        };
        view.render_ms = first;
        view.advance(first, 0.0);
        view.started = true;
    }
    // Who is posed this frame, chosen before the steps: they read their
    // torso's motion as they steer. The camera where it was last frame, in
    // game space (metres).
    let eye = cameras
        .iter()
        .next()
        .and_then(|(entity, _)| transforms.get(entity).ok())
        .map_or(Vec3::ZERO, |t| game_to_bevy().inverse() * t.translation);
    view.close = view.cast.nearest(&view.history, target, eye, view.me);
    let shown_ms = view.render_ms;
    while view.render_ms + 1e-6 < target {
        let next = (view.render_ms + STEP_MS).min(target);
        let frame = (next - view.render_ms) as f32;
        view.render_ms = next;
        view.advance(next, frame);
    }
    // Each figure's whooshes as its weapon sounds (`weapon_sounds`).
    if let Some(newest) = view.history.last() {
        let objects = Instances::in_snapshot(&view.level, newest);
        for (keyed, at) in view.keyed.drain(..) {
            let kotor = kotor.as_deref().map(|kotor| {
                let entity = cast::entity(newest, at.0);
                (kotor, entity.and_then(|e| kotor.row(e, newest, &objects)))
            });
            cues.0.extend(crate::blade_fx::keyframed(&keyed, at, kotor));
        }
    }
    let found = std::mem::take(&mut view.own.contacts);
    if let Some(contacts) = contacts.as_deref_mut() {
        contacts.0.extend(found);
    }
    if let Some(fire) = fire.as_deref_mut() {
        publish_fire(view, fire);
    }
    // Keep what the moment is blended from, and the blades' last strokes
    // (where a killing one went through, `ragdolls`).
    let ms = view.render_ms;
    let old = view
        .history
        .iter()
        .take_while(|s| (s.time_ms as f64) < ms - crate::ragdolls::KEPT_MS)
        .count();
    view.history.drain(..old);
    let own = view.own.at(view.me, ms);
    // Where the sim's blades are, for the hands that grip them to reach.
    let reach = view
        .close
        .iter()
        .map(|&id| {
            let blades = sim_blades::in_hands(&view.level, (&view.history, ms), id, own.as_ref());
            (id, blades)
        })
        .filter(|(_, blades)| !blades.is_empty())
        .collect();
    // The own gun held up before the eyes the player looked through last
    // frame (`first_person`).
    view.cast.eye = view.first.shown();
    let posed = view.cast.draw(
        (&view.history, &view.level),
        ms,
        (&view.close, &reach),
        view.me,
        own.as_ref(),
        &mut transforms,
        &mut Props {
            visibility: &mut visibility,
            lives: &mut lives,
            commands: &mut commands,
            root: root.iter().next(),
        },
    );
    // What the bodies play keys their emitters (`own_fx`).
    if let Some(body_clips) = &mut body_clips {
        body_clips.0 = std::mem::take(&mut view.cast.body_clips);
    }
    let staged = stage_stunts(view, ms, &scene.0, &mut transforms);
    // What KOTOR's clips' own events bring as drawn (`clip_events`): the
    // scripts' and rounds' clips, and a cutscene's stunts.
    let mut heard = view.cast.heard();
    heard.extend(staged);
    if !heard.is_empty()
        && let (Some(kotor), Some(newest)) = (kotor.as_deref(), view.history.last())
    {
        let objects = Instances::in_snapshot(&view.level, newest);
        for (id, heard) in heard {
            if heard == crate::clip_events::Heard::Contact {
                hooks.iter_mut().for_each(|hooks| {
                    hooks.0.push(crate::script_fx::HookVisual {
                        row: crate::clip_events::CONTACT_SPARKS,
                        place: crate::script_fx::Place::Hook {
                            on: id,
                            hook: crate::clip_events::SPARKS_HOOK,
                        },
                    })
                });
                continue;
            }
            let (Some(entity), Some(at)) = (
                cast::entity(newest, id),
                cast::blended(&view.history, id, ms),
            ) else {
                continue;
            };
            if heard == crate::clip_events::Heard::Footstep {
                steps.0.push((id, Vec3::from_array(at.origin)));
                continue;
            }
            let row = kotor.in_hand(entity, &objects);
            let at = (id, Vec3::from_array(at.origin), ms as f32);
            cues.0
                .extend(crate::clip_events::cue(heard, at, (kotor, row)));
        }
    }
    // The cuts the deaths drawn just now made: parted this frame, as their
    // limbs first fly (`severing::apply`, after this system).
    cuts.0.append(&mut view.cast.cuts);
    if let Some(sim) = sim_blades.as_deref_mut() {
        sim.ms = ms;
        sim.shown = sim_blades::in_hand(
            &view.cast,
            &view.level,
            (&view.history, ms),
            own.as_ref(),
            &view.close,
        );
    }
    let Some(at) = view.subject(ms, own.as_ref()) else {
        // No one to look through: the own head shown, its flashes whole.
        view.first.show(&mut visibility);
        eyes.0 = None;
        return;
    };

    let soles = cast::soles(at.origin);
    if let (Some((file, traced)), Some(posed), Some(rig)) =
        (&mut view.trace, &posed, view.cast.player_rig())
    {
        let correction = Vec3::from_array(view.own.correction).length();
        write_trace(file, traced, rig, posed, (soles, ms, correction), &at.state);
    }
    // A watching seat (no body of its own, or down) aims at nothing.
    let watching = view.watched != view.me;
    // Knight and Master Speed widen the view (`script_fx::OwnScreen`); a
    // screen shake turns it.
    let fov = screen
        .as_ref()
        .map_or(FOV_DEGREES, |screen| screen.fov(FOV_DEGREES, ms));
    let shaken = screen
        .and_then(|screen| screen.shake)
        .map_or([0.0; 2], |shake| {
            let roll = crate::view_rng::mix32_salted(ms.to_bits() as u32, 0x5ac3);
            shake.angles(ms, [roll & 1 == 1, roll & 2 == 2])
        });
    let talk = (!view.fixed)
        .then(|| {
            talk_camera(
                view,
                seat.0,
                (soles, ms),
                &TalkScene {
                    placed: &scene.0.cameras,
                    pull_backs: &scene.0.pull_backs,
                    models: &scene.0.cutscene_models,
                    drawn: &|node| {
                        globals
                            .get(node)
                            .ok()
                            .map(|placed| game_to_bevy().inverse() * placed.translation())
                    },
                },
            )
        })
        .flatten();
    // The own eyes while the player looks through them: its own body, not
    // watching (down or bodiless), no conversation or `camera` command
    // holding the view.
    let body = view
        .me
        .filter(|_| view.camera_mode == CameraMode::First && talk.is_none())
        .filter(|_| !watching && !view.fixed)
        .and_then(|me| view.cast.drawn_body(me)?.kotor);
    let first = body.and_then(|body| {
        let mut eye_height = at.origin;
        eye_height[2] += at.state.eye_height;
        let angles = at.view;
        let drawn = first_person::Drawn {
            place: &|entity| first_person::place(entity, &transforms, &parents),
            hung: &|node| first_person::hung(node, &children, &scene_models),
            scene: &scene.0,
        };
        let looking = first_person::Looking {
            body,
            eye_height: Vec3::from_array(eye_height) * UNIT,
            angles,
            blade: view
                .me
                .and_then(|me| view.cast.figure(me))
                .is_some_and(cast::Figure::blade),
            stepped: view.me.map_or(0.0, |me| {
                first_person::stepped(&view.history, me, (shown_ms, ms))
            }),
            dt: time.delta_secs(),
        };
        let eye = view.first.eye(&looking, &drawn, &mut visibility)?;
        let eye = stop_short(
            (&view.level, view.doors.as_deref()),
            eye_height,
            (eye / UNIT).to_array(),
        );
        Some((eye.map(|x| x * UNIT), angles))
    });
    if first.is_none() {
        view.first.show(&mut visibility);
    }
    eyes.0 = view.me.filter(|_| first.is_some());
    aim.0 = place_camera(
        view,
        (talk, first),
        (soles, fov, shaken),
        &mut transforms,
        &mut cameras,
    )
    .filter(|_| !watching);
    // The own commands' crosshair line: the camera's ray off the player's
    // third-person line, while a camera of the player's looks along its
    // view (its eyes', JA's); none for a conversation's or a fixed one.
    camera_aim.0 = aim
        .0
        .filter(|_| talk.is_none() && !view.fixed)
        .map_or([0; 3], |look| {
            let player = sim::motion::MotionState {
                origin: at.origin,
                view: at.view,
                ..at.state
            };
            sim::combat::mind::aim_for(&player, look.from.map(|metres| metres / UNIT))
        });
    view.frames.push_back(time.delta_secs() * 1000.0);
    if view.frames.len() > FRAMES_KEPT {
        view.frames.pop_front();
    }
    let frames = view.frames.len().max(1) as f32;
    let p = at.state;
    facts.0.insert(
        "play".into(),
        serde_json::json!({
            "render_ms": ms,
            "origin_units": at.origin,
            "velocity": p.velocity,
            "view": at.view,
            "ground": p.ground.is_some(),
            "ducked": p.ducked(),
            "legs": p.legs.anim.name(),
            "torso": p.torso.anim.name(),
            "legs_yaw": view.me.and_then(|me| view.cast.figure(me)?.yaw()),
            "camera_units": view.camera.location,
            "camera_target_units": view.camera.target,
            "soles_m": soles.to_array(),
            "watching": view.watched != view.me,
            "follows": view.watched.map(|id| id.0),
            "own_rewrites": view.own.rewrites,
            "own_rewritten": view.own.rewritten,
        }),
    );
    let (weapons, blades_lit) = view.cast.arms_drawn();
    facts.0.insert(
        "characters".into(),
        serde_json::json!({
            "drawn": view.cast.drawn(),
            "seat_bodies_shown": seat_bodies,
            "weapons_shown": weapons,
            "blades_lit": blades_lit,
            "posed": view.cast.posed,
            "pose_ms": view.cast.pose_ms,
            "arms_ms": view.cast.arms_ms,
            "ragdolls": view.cast.ragdolls.counts(),
            "lying": fading.lying(),
            "ragdoll_ms": view.cast.ragdolls.step_ms,
            "falls": dump_due.0.then(|| view.cast.ragdolls.facts()),
            "frame_ms_mean": view.frames.iter().sum::<f32>() / frames,
            "frame_ms_worst": view.frames.iter().copied().fold(0.0, f32::max),
            "frames": view.frames.len(),
            // Only for a dump: one row per character.
            "roster": dump_due.0.then(|| view.cast.roster(&view.history, ms, &view.level)),
        }),
    );
}

/// Hands `combat_fx` the own shots and bolts found since the last frame,
/// the own time's lead, and the others' bodies as drawn now.
fn publish_fire(view: &mut View, fire: &mut OwnFire) {
    let me = view.me.filter(|_| view.own.active);
    fire.me = me;
    fire.shift = me.map(|_| view.own.shift);
    fire.shots.append(&mut view.own.shots);
    fire.bolts.append(&mut view.own.bolts);
    fire.bodies.clear();
    let own_ms = view.render_ms + view.own.shift;
    let shown = view
        .own
        .history
        .iter()
        .rev()
        .find(|s| s.time_ms as f64 <= own_ms)
        .or(view.own.history.first());
    let is = me.and_then(|me| shown.and_then(|s| cast::entity(s, me)));
    fire.own = is.copied();
    if let Some(is) = is {
        let ms = view.render_ms;
        fire.bodies = bodies_as_drawn(&view.level, (&view.history, ms), is, |_| true);
    }
}

/// One line of the trace: the body's traced nodes in the world, as the
/// scene places them, and a joined player's correction (units).
fn write_trace(
    file: &mut std::io::BufWriter<std::fs::File>,
    traced: &[usize],
    rig: &BodyRig,
    posed: &asset_ja::rig::Posed,
    (soles, ms, correction): (Vec3, f64, f32),
    state: &motion::MotionState,
) {
    use std::io::Write as _;
    let rotations: BTreeMap<usize, Quat> = posed.rotations.iter().copied().collect();
    let base = Transform::from_translation(soles)
        .with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2));
    let mut world: Vec<Transform> = Vec::with_capacity(rig.rest.len());
    for (index, &(parent, rest, rest_rotation)) in rig.rest.iter().enumerate() {
        let mut local = Transform::from_translation(rest)
            .with_rotation(*rotations.get(&index).unwrap_or(&rest_rotation));
        if let Some((root, offset)) = posed.root
            && root == index
        {
            local.translation = rest + rig.animation_scale * offset;
        }
        let parent = parent.and_then(|p| world.get(p).copied()).unwrap_or(base);
        world.push(parent * local);
    }
    let drawn = traced
        .iter()
        .map(|&n| world.get(n).map_or(Vec3::ZERO, |t| t.translation));
    let ja = posed.ja_arms.iter().map(|&p| base.transform_point(p));
    let cells: Vec<String> = drawn
        .chain(ja)
        .flat_map(|p| [p.x, p.y, p.z].map(|v| format!("{v:.4}")))
        .collect();
    let _ = writeln!(
        file,
        "{ms:.2},{},{},{},{correction:.2}",
        state.legs.anim.name(),
        state.torso.anim.name(),
        cells.join(",")
    );
    let _ = file.flush();
}

/// How far from its soles a `camerahook` may be taken (metres): farther,
/// the node is not placed yet.
const HOOK_REACH: f32 = 4.0;
/// The dialog camera's field of view (degrees): KOTOR's at every shot it
/// sets up, and an animated line's when it sets none (Q-0141).
const DIALOG_FOV: f32 = 55.0;
/// The first `AnimList` value naming a `dialoganimations.2da` row.
const FIRST_DIALOG_ANIMATION: u16 = 10000;
/// The pairs whose side the dialog camera remembers, each conversation.
const PAIRS_KEPT: usize = 4;

/// A line's `CameraAngle` as its conversation resolved it (S6, Q-0230): the
/// close-up of the speaker, over the listener's shoulder, the wide two-shot,
/// no shot of its own, the shot kept, and a placed camera.
const ANGLE_CLOSE: u8 = 1;
const ANGLE_SHOULDER: u8 = 2;
const ANGLE_WIDE: u8 = 3;
const ANGLE_NONE: u8 = 4;
const ANGLE_KEEP: u8 = 5;
const ANGLE_PLACED: u8 = 6;

/// Cutscene stunt clips by stunt model and clip name (`None`: the model
/// has no such clip).
type StuntClips = BTreeMap<(String, String), Option<Arc<AnimationIr>>>;

/// A camera's place (game space, metres), view angles and vertical field
/// of view (degrees).
pub type TalkShot = ([f32; 3], [f32; 3], f32);

/// The own eyes' place (game space, metres) and view angles (degrees).
type EyeShot = ([f32; 3], [f32; 3]);

/// What a conversation's camera is placed from: the area's cameras, the
/// dialog animations' close-up pull-backs, the conversations' camera
/// models, the scene as last drawn.
struct TalkScene<'a> {
    placed: &'a [PlacedCamera],
    /// `dialoganimations.2da`'s `CU_PB_Range` by row
    /// (`LoadedScene::pull_backs`).
    pull_backs: &'a [f32],
    models: &'a BTreeMap<String, Arc<ModelIr>>,
    /// Where a node entity was drawn last frame (game space, metres).
    drawn: &'a dyn Fn(Entity) -> Option<Vec3>,
}

/// The conversation `seat`'s screens show (`dialogue_view::shown`): while
/// it watches, the one holding the party member its camera follows.
fn talk_shown(view: &View, seat: sim::Seat) -> Option<&sim::script::dialogue::Conversation> {
    crate::dialogue_view::shown(view.history.last()?, seat, view.watching)
}

/// Where the camera stands, what it looks at and how wide while `seat`'s
/// screen shows a conversation (`talk_shown`), in KOTOR's order (S6): its
/// camera model's clip for the line (`animated_shot`), else the area's
/// placed camera a script cut to or the line names (`line_camera`), else
/// KOTOR's dialog camera on the line's pair as the line's start set it up
/// (`DialogShot::set_up`), following them as they move. A DLG with a
/// camera model films its lines without a clip the same way. The line's
/// camera fields are its entry's, while the replies are offered the first
/// reply's. Its pair is the line's (`cast::line_pair`): the player is
/// filmed like any speaker.
fn talk_camera(
    view: &View,
    seat: sim::Seat,
    (soles, ms): (Vec3, f64),
    scene: &TalkScene,
) -> Option<TalkShot> {
    let latest = view.history.last()?;
    let talk = talk_shown(view, seat)?;
    let mut current = view
        .dialog_shot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut sides = view
        .sides
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    line_shot(
        (&view.level, latest, talk, ms),
        (scene.placed, scene.pull_backs, scene.models),
        (&mut current, &mut sides),
        &|id| head_point(view, id, (soles, ms), scene.drawn),
        &|from, to| blocked(view, from, to),
    )
    .map(|(_, shot)| shot)
}

/// Where a line is filmed from (`talk_camera`), and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShotKind {
    /// Its camera model's clip.
    Animated,
    /// The area's placed camera of this id.
    Placed(i32),
    /// KOTOR's dialog camera at this angle (1 to 3), on this pair.
    Dialog(u8, (SimId, SimId)),
}

/// `talk`'s shot in `latest` at `ms` (`talk_camera`, which see), the
/// dialog shot under way kept in `current` and the sides chosen in `sides`;
/// the bodies' heads as `head` has them, the walls as `walls` does.
fn line_shot(
    (level, latest, talk, ms): (
        &sim::Level,
        &Snapshot,
        &sim::script::dialogue::Conversation,
        f64,
    ),
    (placed, pull_backs, models): (&[PlacedCamera], &[f32], &BTreeMap<String, Arc<ModelIr>>),
    (current, sides): (&mut Option<DialogShot>, &mut Sides),
    head: &dyn Fn(SimId) -> Option<Vec3>,
    walls: &dyn Fn(Vec3, Vec3) -> bool,
) -> Option<(ShotKind, TalkShot)> {
    // An animated or a placed line leaves the last dialog shot as it was,
    // so a line of angle 5 or 4 after one keeps that shot (`kept`)
    // [verify: the exe likely makes the previous line's pair face each
    // other again instead].
    if let Some(shot) = animated_shot(level, talk, models, ms) {
        return Some((ShotKind::Animated, shot));
    }
    let dlg = level
        .scripts()
        .and_then(|scripts| scripts.dialogue(&talk.dialog));
    let cast::LinePair {
        speaker,
        listener,
        player,
        offered,
    } = cast::line_pair(latest, talk);
    let node = dlg.and_then(|dlg| match offered {
        true => dlg.replies.get(*talk.replies.first()? as usize),
        false => dlg.entries.get(talk.entry as usize),
    });
    let named = node
        .filter(|_| talk.angle == ANGLE_PLACED)
        .map(|node| node.camera_id);
    if let Some(camera) = line_camera(talk, named, placed) {
        return Some((ShotKind::Placed(camera.id), placed_shot(camera)));
    }
    let line = (talk.id, talk.shown_ms, offered);
    let shot = match *current {
        Some(shot) if shot.line == line => shot,
        before => {
            let start = LineStart {
                line,
                angle: talk.angle,
                pair: (speaker, listener),
                heads: (head(speaker)?, head(listener)?),
                heights: node.map_or((0.0, 0.0), |node| (node.cam_height, node.target_height)),
                pull_back: node.and_then(|node| {
                    speaker_pull_back((latest, level), talk, node, (speaker, player), pull_backs)
                }),
            };
            let shot = DialogShot::set_up(&start, before, sides, walls);
            *current = Some(shot);
            shot
        }
    };
    let (eye, target) = shot.film(head(shot.pair.0)?, head(shot.pair.1)?);
    let angle = if shot.close { ANGLE_CLOSE } else { shot.angle };
    Some((
        ShotKind::Dialog(angle, shot.pair),
        (
            eye.to_array(),
            angles::look_angles((target - eye).to_array()),
            DIALOG_FOV,
        ),
    ))
}

/// The shots a conversation's lines are filmed from, followed line by line
/// as the view follows them (`line_shot`), for `kjk dialogue check`.
#[cfg(feature = "probes")]
#[derive(Default)]
pub struct LineShots {
    current: Option<DialogShot>,
    sides: Sides,
}

#[cfg(feature = "probes")]
impl LineShots {
    /// `talk`'s shot in `latest` at `ms` (sim ms): how it is filmed, the
    /// eye (game space, metres), its view angles and its vertical field of
    /// view (degrees).
    pub fn shot(
        &mut self,
        (level, latest, talk, ms): (
            &sim::Level,
            &Snapshot,
            &sim::script::dialogue::Conversation,
            f64,
        ),
        (placed, pull_backs, models): (&[PlacedCamera], &[f32], &BTreeMap<String, Arc<ModelIr>>),
        head: &dyn Fn(SimId) -> Option<Vec3>,
        walls: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> Option<(ShotKind, TalkShot)> {
        line_shot(
            (level, latest, talk, ms),
            (placed, pull_backs, models),
            (&mut self.current, &mut self.sides),
            head,
            walls,
        )
    }
}

/// Where `id`'s head is for the dialog camera (game space, metres): its
/// `camerahook` as last drawn (`Cast::camera_node`), else its soles raised
/// by its model's height (`Cast::model_height`, the top of its meshes at
/// rest; KOTOR raises the body's position by 0.1 m and a height of the
/// model's own [verify]), else by a human's (`cast::HEAD_M`). The own body
/// stands at `soles`, as predicted.
fn head_point(
    view: &View,
    id: SimId,
    (soles, ms): (Vec3, f64),
    drawn: &dyn Fn(Entity) -> Option<Vec3>,
) -> Option<Vec3> {
    let feet = if view.me == Some(id) {
        soles
    } else {
        match cast::blended(&view.history, id, ms) {
            Some(at) => cast::soles(at.origin),
            None => Vec3::from_array(cast::entity(view.history.last()?, id)?.position),
        }
    };
    let hook = view.cast.camera_node(id).and_then(drawn);
    let height = view.cast.model_height(id).unwrap_or(cast::HEAD_M);
    Some(
        hook.filter(|hook| hook.distance(feet) <= HOOK_REACH)
            .unwrap_or(feet + Vec3::Z * height),
    )
}

/// The close-up pull-back (metres) of the talk clip line `node`'s
/// `AnimList` gives `speaker` (`player` the list's "player"): its
/// `dialoganimations.2da` row's `CU_PB_Range` (`pull_backs`), the row the
/// value less 10000; none where it gives the speaker no talk clip. Under a
/// millimetre is 0.
fn speaker_pull_back(
    (latest, level): (&Snapshot, &sim::Level),
    talk: &sim::script::dialogue::Conversation,
    node: &fmt_odyssey::dlg::Node,
    (speaker, player): (SimId, SimId),
    pull_backs: &[f32],
) -> Option<f32> {
    let objects = Instances::in_snapshot(level, latest);
    let (_, value) = node.anims.iter().find(|(tag, _)| {
        let who = sim::script::dialogue::participant(tag, (SimId(talk.owner.0), player), |tag| {
            cast::tagged(latest, &objects, tag)
        });
        who == Some(speaker)
    })?;
    let row = value.checked_sub(FIRST_DIALOG_ANIMATION)?;
    let pull_back = pull_backs.get(usize::from(row)).copied().unwrap_or(0.0);
    Some(if pull_back.abs() < 0.001 {
        0.0
    } else {
        pull_back
    })
}

/// What a line's start gives the dialog camera (S6): the line
/// (conversation, when it showed, whether the replies are up), its
/// resolved `CameraAngle`, its pair (A, B) and their heads (game space,
/// metres), its `CamHeightOffset` and `TarHeightOffset`, and the speaker's
/// close-up pull-back, none where its talk clip gives none.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LineStart {
    line: (u32, u64, bool),
    angle: u8,
    pair: (SimId, SimId),
    heads: (Vec3, Vec3),
    heights: (f32, f32),
    pull_back: Option<f32>,
}

/// KOTOR's dialog camera as a line's start sets it up (S6): the line, the
/// angle asked (1 to 3), the pair it films (A, B), its side (±1,
/// `Sides::side`), whether a wall on its sight line dropped it to the
/// close-up, the speaker's close-up pull-back, and the line's offsets. Each
/// frame films the pair where they stand (`film`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct DialogShot {
    line: (u32, u64, bool),
    angle: u8,
    pair: (SimId, SimId),
    side: f32,
    close: bool,
    pull_back: f32,
    heights: (f32, f32),
}

impl DialogShot {
    /// The shot `start` sets up, the shot `before` it under way, the sides
    /// chosen so far `sides`, and whether a wall stands between two points
    /// as `walls` says:
    /// - a kept shot carries on (`kept`);
    /// - else the angle (any other but 2 and 3 the close-up) on the pair, on
    ///   the pair's side; where a wall stands between the eye and the
    ///   speaker's head (for the wide shot the midpoint or either head), the
    ///   close-up with no pull-back for the whole line. The eye is never
    ///   pulled in. The pull-back is the speaker's talk clip's, else the one
    ///   before carries on.
    fn set_up(
        start: &LineStart,
        before: Option<DialogShot>,
        sides: &mut Sides,
        walls: &dyn Fn(Vec3, Vec3) -> bool,
    ) -> DialogShot {
        let before = before.filter(|shot| shot.line.0 == start.line.0);
        if let Some(shot) = kept(start.angle, before) {
            return DialogShot {
                line: start.line,
                ..shot
            };
        }
        let angle = match start.angle {
            ANGLE_SHOULDER | ANGLE_WIDE => start.angle,
            _ => ANGLE_CLOSE,
        };
        let (a, b) = start.heads;
        let side = sides.side(start.line.0, start.pair, |side| {
            blocked_lines(start.heads, side, start.heights, walls)
        });
        let shot = DialogShot {
            line: start.line,
            angle,
            pair: start.pair,
            side,
            close: false,
            pull_back: start
                .pull_back
                .or(before.map(|shot| shot.pull_back))
                .unwrap_or(0.0),
            heights: start.heights,
        };
        let (eye, _) = shot.film(a, b);
        let sights = match angle {
            ANGLE_WIDE => vec![(a + b) / 2.0, a, b],
            _ => vec![a],
        };
        let close = sights.into_iter().any(|to| walls(eye, to));
        DialogShot { close, ..shot }
    }

    /// The eye and what it looks at (game space, metres) with A's and B's
    /// heads at `a` and `b`: the close-up with no pull-back where a wall
    /// dropped it.
    fn film(&self, a: Vec3, b: Vec3) -> (Vec3, Vec3) {
        let (angle, pull_back) = if self.close {
            (ANGLE_CLOSE, 0.0)
        } else {
            (self.angle, self.pull_back)
        };
        let (cam_h, tar_h) = self.heights;
        dialog_shot(angle, a, b, self.side, pull_back, cam_h, tar_h)
    }
}

/// The shot `before` (of the same conversation) a line of angle `angle`
/// keeps, pair and all: 5 keeps it, and so does 4 (KOTOR's dialog camera
/// has no shot for it [verify]); 3 after 3 keeps it too. None for the
/// others, or with no shot before. After an animated or a placed line,
/// `before` is the last dialog shot ahead of it [verify: the exe likely
/// has the previous line's pair face each other again].
fn kept(angle: u8, before: Option<DialogShot>) -> Option<DialogShot> {
    match angle {
        ANGLE_NONE | ANGLE_KEEP => before,
        ANGLE_WIDE => before.filter(|shot| shot.angle == ANGLE_WIDE),
        _ => None,
    }
}

/// The sides the dialog camera chose for the pairs of conversation `id`:
/// side 1 or 2 by pair (A, B), at most `PAIRS_KEPT`.
#[derive(Debug, Default)]
struct Sides {
    id: Option<u32>,
    pairs: Vec<((SimId, SimId), u8)>,
}

impl Sides {
    /// The side sign for `pair` (A, B) in conversation `id` (S6): the side
    /// chosen for the pair before, or for the pair reversed the other one,
    /// which keeps the camera on the same side of the line between them
    /// (the 180° rule); else the side whose shots meet fewer walls
    /// (`blocked` by side sign, `blocked_lines`), side 2 on a tie,
    /// remembered in one of `PAIRS_KEPT` slots, the first overwritten once
    /// all are used. Another conversation starts afresh. Side 1 turns the
    /// shots by +30° and +90°, side 2 by −30° and −90°.
    fn side(&mut self, id: u32, pair: (SimId, SimId), blocked: impl Fn(f32) -> usize) -> f32 {
        let sign = |side: u8| if side == 1 { 1.0 } else { -1.0 };
        if self.id != Some(id) {
            *self = Sides {
                id: Some(id),
                pairs: Vec::new(),
            };
        }
        let known = self.pairs.iter().find_map(|&(seen, side)| {
            if seen == pair {
                Some(side)
            } else if seen == (pair.1, pair.0) {
                Some(3 - side)
            } else {
                None
            }
        });
        if let Some(side) = known {
            return sign(side);
        }
        let side = if blocked(sign(1)) < blocked(sign(2)) {
            1
        } else {
            2
        };
        if self.pairs.len() < PAIRS_KEPT {
            self.pairs.push((pair, side));
        } else {
            self.pairs[0] = (pair, side);
        }
        sign(side)
    }
}

/// How many of the sight lines KOTOR tries for side `side` of the pair
/// whose heads are at `a` and `b` meet a wall (`walls`): the wide shot's to
/// their midpoint and to both heads, the over-the-shoulder shot of A's to
/// A's head, and of B's (the pair reversed, the other side) to B's.
fn blocked_lines(
    (a, b): (Vec3, Vec3),
    side: f32,
    (cam_h, tar_h): (f32, f32),
    walls: &dyn Fn(Vec3, Vec3) -> bool,
) -> usize {
    let eye = |angle, (x, y), side| dialog_shot(angle, x, y, side, 0.0, cam_h, tar_h).0;
    let wide = eye(ANGLE_WIDE, (a, b), side);
    let lines = [
        (wide, (a + b) / 2.0),
        (wide, a),
        (wide, b),
        (eye(ANGLE_SHOULDER, (a, b), side), a),
        (eye(ANGLE_SHOULDER, (b, a), -side), b),
    ];
    lines
        .into_iter()
        .filter(|&(from, to)| walls(from, to))
        .count()
}

/// Whether the level's walls (doors as drawn) stand on the sight line from
/// `eye` to `to` (game space, metres). The level holds no creatures, so
/// the line passes through bodies, as KOTOR's ignores the pair's.
fn blocked(view: &View, eye: Vec3, to: Vec3) -> bool {
    let hit = view.level.with(view.doors.as_deref(), |world| {
        world.trace(
            eye.to_array(),
            to.to_array(),
            &Extents::default(),
            Contents::SOLID,
        )
    });
    hit.fraction < 1.0
}

/// KOTOR's dialog camera's eye and target (game space, metres) for angle
/// `angle`, A's and B's head points `a` and `b`, side `side` (±1), the
/// speaker's close-up pull-back, and the line's `CamHeightOffset`
/// (`cam_h`) and `TarHeightOffset` (`tar_h`); the exe's constants (S6).
/// rotZ(θ) turns about the vertical by θ·`side`, L is |A − B|:
/// - 1, the close-up of A: B levelled to A's height (B′); the target 0.2 m
///   from A toward B′, 0.04 m under A, lowered by 0.2·pull-back and raised
///   by `tar_h`; the eye (0.5 + pull-back) m behind it along
///   unit(rotZ(30°)(A − B′)), 0.04 m under A, raised by both offsets;
/// - 2, over B's shoulder at A: A lowered by 0.1·L and B by 0.05·L; the
///   target 30% of the way from B to A, raised by `tar_h`; the eye
///   0.8·v + 0.15·unit(v) behind it, v = rotZ(30°)(A − B) as lowered,
///   raised by `cam_h`;
/// - 3, the two side on: the target their midpoint lowered by 0.2·L and
///   raised by `tar_h`; the eye 1.5·rotZ(90°)(A − B) behind it, raised by
///   0.3·L and `cam_h`.
///
/// Any other angle is the close-up. A and B at one point film A close up
/// with no pull-back, B taken at A + (1.5, 1.5, 0.1).
fn dialog_shot(
    angle: u8,
    a: Vec3,
    b: Vec3,
    side: f32,
    pull_back: f32,
    cam_h: f32,
    tar_h: f32,
) -> (Vec3, Vec3) {
    let (angle, b, pull_back) = if a == b {
        (ANGLE_CLOSE, a + Vec3::new(1.5, 1.5, 0.1), 0.0)
    } else {
        (angle, b, pull_back)
    };
    let turn = |degrees: f32, v: Vec3| Quat::from_rotation_z((degrees * side).to_radians()) * v;
    let l = a.distance(b);
    match angle {
        ANGLE_SHOULDER => {
            let (a, b) = (a - Vec3::Z * 0.1 * l, b - Vec3::Z * 0.05 * l);
            let target = b + 0.3 * (a - b) + Vec3::Z * tar_h;
            let v = turn(30.0, a - b);
            let eye = target - (0.8 * v + 0.15 * v.normalize_or_zero()) + Vec3::Z * cam_h;
            (eye, target)
        }
        ANGLE_WIDE => {
            let target = (a + b) / 2.0 + Vec3::Z * (tar_h - 0.2 * l);
            let eye = target - 1.5 * turn(90.0, a - b) + Vec3::Z * (0.3 * l + cam_h);
            (eye, target)
        }
        _ => {
            let level = Vec3::new(b.x, b.y, a.z);
            let height = a.z - 0.04 + tar_h;
            let target =
                (a - 0.2 * (a - level).normalize_or_zero()).with_z(height - 0.2 * pull_back);
            let u = turn(30.0, a - level).normalize_or_zero();
            let eye = (target - (0.5 + pull_back) * u).with_z(height + cam_h);
            (eye, target)
        }
    }
}

/// The dialog camera's geometry (`dialog_shot`) against the exe's
/// formulas, A's head at (0, 0, 1.7) and B's at (2, 0, 1.7), side +1, so
/// L = 2 and A − B = (−2, 0, 0), to a millimetre:
/// - close-up: the target 0.2 m from A toward B and 0.04 m down,
///   (0.2, 0, 1.66); the eye 0.5 m behind it along
///   unit(rotZ(30°)(A − B)) = (−0.866, −0.5, 0): (0.6330, 0.25, 1.66);
/// - the same with a pull-back of 0.3 and offsets of 0.1 (camera) and 0.05
///   (target): the target 0.06 lower and 0.05 up, (0.2, 0, 1.65); the eye
///   0.8 m behind at 1.66 + 0.15: (0.8928, 0.4, 1.81);
/// - over the shoulder: A lowered to 1.5 and B to 1.6, the target 30% of
///   the way, (1.4, 0, 1.57); v = rotZ(30°)(−2, 0, −0.1) =
///   (−1.7321, −1, −0.1), |v| = 2.0025; the eye 0.8·v + 0.15·unit(v)
///   behind it: (2.9154, 0.8749, 1.6575);
/// - wide: the target the midpoint 0.4 m down, (1, 0, 1.3); the eye
///   1.5·rotZ(90°)(A − B) = (0, −3, 0) behind it and 0.6 m up: (1, 3, 1.9);
/// - the pair reversed on the other side: the same wide shot (the 180°
///   rule);
/// - A and B at one point, over the shoulder with a pull-back: the
///   close-up of A toward A + (1.5, 1.5, 0.1), no pull-back.
#[cfg(feature = "probes")]
pub fn shot_probes() -> sim::probe::Checks {
    let (a, b) = (Vec3::new(0.0, 0.0, 1.7), Vec3::new(2.0, 0.0, 1.7));
    let wide = (Vec3::new(1.0, 3.0, 1.9), Vec3::new(1.0, 0.0, 1.3));
    let cases = [
        (
            "dialog_close_up",
            dialog_shot(ANGLE_CLOSE, a, b, 1.0, 0.0, 0.0, 0.0),
            (Vec3::new(0.633_013, 0.25, 1.66), Vec3::new(0.2, 0.0, 1.66)),
        ),
        (
            "dialog_close_up_pull_back",
            dialog_shot(ANGLE_CLOSE, a, b, 1.0, 0.3, 0.1, 0.05),
            (Vec3::new(0.892_820, 0.4, 1.81), Vec3::new(0.2, 0.0, 1.65)),
        ),
        (
            "dialog_over_shoulder",
            dialog_shot(ANGLE_SHOULDER, a, b, 1.0, 0.0, 0.0, 0.0),
            (
                Vec3::new(2.915_383, 0.874_906, 1.657_491),
                Vec3::new(1.4, 0.0, 1.57),
            ),
        ),
        (
            "dialog_wide",
            dialog_shot(ANGLE_WIDE, a, b, 1.0, 0.0, 0.0, 0.0),
            wide,
        ),
        (
            "dialog_wide_reversed",
            dialog_shot(ANGLE_WIDE, b, a, -1.0, 0.0, 0.0, 0.0),
            wide,
        ),
        (
            "dialog_same_point",
            dialog_shot(ANGLE_SHOULDER, a, a, 1.0, 0.3, 0.0, 0.0),
            dialog_shot(
                ANGLE_CLOSE,
                a,
                a + Vec3::new(1.5, 1.5, 0.1),
                1.0,
                0.0,
                0.0,
                0.0,
            ),
        ),
    ];
    cases
        .into_iter()
        .map(|(name, (eye, target), (want_eye, want_target))| {
            let off = eye.distance(want_eye).max(target.distance(want_target));
            let detail =
                format!("eye {eye:.4}, target {target:.4} (want {want_eye:.4}, {want_target:.4})");
            (name, sim::probe::check(off < 0.001, detail))
        })
        .chain(rule_probes())
        .collect()
}

/// The dialog camera's rules (`DialogShot::set_up`, `kept`, `Sides::side`)
/// on a pair whose heads stand at (0, 0, 1.7) and (2, 0, 1.7), walls as
/// each case says:
/// - `side_tie`: no walls: side 2 (−1);
/// - `side_walls`: walls on every sight line from south of the pair (side
///   2's eyes): side 1 (+1);
/// - `side_reversed`: the pair reversed takes the other side, and the pair
///   again its own, whatever the walls say then;
/// - `side_slots`: five pairs: the fifth in the first slot, the second to
///   fourth kept; another conversation starts afresh;
/// - `keep_rules`: 5 and 4 keep the shot before and its pair, 3 after 3
///   keeps it; 3 after 1, 1 after 3 and nothing before keep none; a shot of
///   another conversation is not kept;
/// - `wall_close_up`: a wall on the sight line drops an over-the-shoulder
///   shot to the close-up with no pull-back;
/// - `pull_back_carries`: a line whose talk clip gives none carries the 0.3
///   before; one giving 0 resets it;
/// - `missing_camera`: a placed line naming camera 3 of the area is filmed
///   from it; one naming camera 9, which the area lacks, from the area's
///   origin looking straight down at 55 degrees; a line naming none has no
///   placed camera.
#[cfg(feature = "probes")]
fn rule_probes() -> sim::probe::Checks {
    let (a, b) = (Vec3::new(0.0, 0.0, 1.7), Vec3::new(2.0, 0.0, 1.7));
    let (pa, pb, pc) = (SimId(1), SimId(2), SimId(3));
    let open = |_: Vec3, _: Vec3| false;
    let none = |_: f32| 0;
    let start = |ms: u64, angle: u8, pull_back: Option<f32>| LineStart {
        line: (7, ms, false),
        angle,
        pair: (pa, pb),
        heads: (a, b),
        heights: (0.0, 0.0),
        pull_back,
    };
    let check = sim::probe::check;
    let mut sides = Sides::default();
    let tie = sides.side(7, (pa, pb), none);
    let south = |side: f32| blocked_lines((a, b), side, (0.0, 0.0), &|from: Vec3, _| from.y < 0.0);
    let walled = Sides::default().side(7, (pa, pb), south);
    let reversed = sides.side(7, (pb, pa), none);
    let again = sides.side(7, (pa, pb), |side| if side < 0.0 { 5 } else { 0 });
    let mut slots = Sides::default();
    for n in 1..=5 {
        slots.side(7, (SimId(n), SimId(10 + n)), none);
    }
    let kept_firsts: Vec<u32> = slots.pairs.iter().map(|(pair, _)| pair.0.0).collect();
    slots.side(8, (pa, pb), none);
    let afresh = slots.id == Some(8) && slots.pairs.len() == 1;
    let set_up = |start: &LineStart, before, walls: &dyn Fn(Vec3, Vec3) -> bool| {
        DialogShot::set_up(start, before, &mut Sides::default(), walls)
    };
    let close = set_up(&start(1, ANGLE_CLOSE, Some(0.3)), None, &open);
    let wide = set_up(&start(2, ANGLE_WIDE, None), Some(close), &open);
    let elsewhere = |angle| LineStart {
        pair: (pc, pa),
        ..start(3, angle, None)
    };
    let keeps = |angle, before: DialogShot| {
        let shot = set_up(&elsewhere(angle), Some(before), &open);
        shot == DialogShot {
            line: (7, 3, false),
            ..before
        }
    };
    let other = DialogShot {
        line: (9, 1, false),
        ..wide
    };
    let rules = [
        keeps(ANGLE_KEEP, close),
        keeps(ANGLE_NONE, close),
        keeps(ANGLE_WIDE, wide),
        kept(ANGLE_WIDE, Some(close)).is_none(),
        kept(ANGLE_CLOSE, Some(wide)).is_none(),
        kept(ANGLE_KEEP, None).is_none(),
        set_up(&elsewhere(ANGLE_KEEP), Some(other), &open).pair == (pc, pa),
    ];
    let walled_off = set_up(&start(4, ANGLE_SHOULDER, Some(0.3)), None, &|_, _| true);
    let close_up = dialog_shot(ANGLE_CLOSE, a, b, walled_off.side, 0.0, 0.0, 0.0);
    let carried = set_up(&start(5, ANGLE_CLOSE, None), Some(close), &open);
    let reset = set_up(&start(6, ANGLE_CLOSE, Some(0.0)), Some(carried), &open);
    let area = [PlacedCamera {
        id: 3,
        position: [1.0, 2.0, 0.0],
        orientation: [1.0, 0.0, 0.0, 0.0],
        pitch: 90.0,
        field_of_view: 40.0,
        height: 1.5,
    }];
    let talk = sim::script::dialogue::Conversation::default();
    let shot = |named| line_camera(&talk, named, &area).map(placed_shot);
    let (found, missing, unnamed) = (shot(Some(3)), shot(Some(9)), shot(None));
    let from = |shot: Option<TalkShot>, eye: [f32; 3], pitch: f32, fov: f32| {
        shot.is_some_and(|(at, [down, ..], wide)| {
            let off = (down - pitch).rem_euclid(360.0);
            at == eye && off.min(360.0 - off) < 0.001 && wide == fov
        })
    };
    vec![
        ("side_tie", check(tie == -1.0, format!("side {tie}"))),
        ("side_walls", check(walled == 1.0, format!("side {walled}"))),
        (
            "side_reversed",
            check(
                reversed == 1.0 && again == -1.0,
                format!("reversed {reversed}, again {again}"),
            ),
        ),
        (
            "side_slots",
            check(
                kept_firsts == [5, 2, 3, 4] && afresh,
                format!(
                    "slots' first bodies {kept_firsts:?}; another conversation afresh {afresh}"
                ),
            ),
        ),
        (
            "keep_rules",
            check(
                rules.iter().all(|ok| *ok),
                format!(
                    "5, 4, 3 after 3 kept; 3 after 1, 1 after 3, none before, another's not: {rules:?}"
                ),
            ),
        ),
        (
            "wall_close_up",
            check(
                walled_off.close && walled_off.film(a, b) == close_up,
                format!(
                    "close {}, filmed {:?} (close-up {close_up:?})",
                    walled_off.close,
                    walled_off.film(a, b)
                ),
            ),
        ),
        (
            "pull_back_carries",
            check(
                carried.pull_back == 0.3 && reset.pull_back == 0.0,
                format!("carried {}, reset {}", carried.pull_back, reset.pull_back),
            ),
        ),
        (
            "missing_camera",
            check(
                from(found, [1.0, 2.0, 1.5], 0.0, 40.0)
                    && from(missing, [0.0; 3], 90.0, DIALOG_FOV)
                    && unnamed.is_none(),
                format!("camera 3 {found:?}, camera 9 {missing:?}, none {unnamed:?}"),
            ),
        ),
    ]
}

/// Where KOTOR's camera films a line of angle 6 whose `CameraID` the area
/// lacks (Q-0230): the area's origin at 55 degrees [likely]; its
/// orientation unturned with no pitch, looking straight down [verify].
const MISSING_CAMERA: PlacedCamera = PlacedCamera {
    id: 0,
    position: [0.0; 3],
    orientation: [1.0, 0.0, 0.0, 0.0],
    pitch: 0.0,
    field_of_view: DIALOG_FOV,
    height: 0.0,
};

/// The placed camera a conversation shows its line from: the one a script
/// cut to, if the area has it (none leaves the line to the dialog camera
/// [verify]), else the one its line `named` (its `CameraID` where its angle
/// is 6), `MISSING_CAMERA` where the area lacks it.
fn line_camera<'a>(
    talk: &sim::script::dialogue::Conversation,
    named: Option<i32>,
    placed: &'a [PlacedCamera],
) -> Option<&'a PlacedCamera> {
    let find = |id: i32| placed.iter().find(|camera| camera.id == id);
    match talk.camera {
        0 => named.map(|id| find(id).unwrap_or(&MISSING_CAMERA)),
        cut => find(cut),
    }
}

/// The shot a conversation's camera model gives its line at `ms`: the
/// model stands at the area's origin (reone), and the camera on its
/// `camerahook` as the line's `CameraAnimation` clip has moved it since the
/// line showed, held at the clip's end; it looks down the hook's own −z.
/// Its field of view is the line's `CamFieldOfView`, else 55 degrees
/// (Q-0141); it may roll. None without a clip (a line with
/// no animation, or one its model lacks): the line then has an ordinary
/// shot, as KOTOR's does.
fn animated_shot(
    level: &sim::Level,
    talk: &sim::script::dialogue::Conversation,
    models: &BTreeMap<String, Arc<ModelIr>>,
    ms: f64,
) -> Option<TalkShot> {
    let dlg = level.scripts()?.dialogue(&talk.dialog)?;
    let model = models.get(&dlg.camera_model.to_ascii_lowercase())?;
    let node = dlg.entries.get(talk.entry as usize)?;
    let name = sim::script::camera_clip_name(node.camera_animation)?;
    let clip = model.animation(&name)?;
    let hook = model.node_index(hooks::CAMERA_HOOK)?;
    let seconds = ((ms - talk.shown_ms as f64) / 1000.0).max(0.0) as f32;
    let posed = posed_node(
        model,
        (clip, model.animation_scale),
        hook,
        seconds.min(clip.length),
    );
    let (_, turn, eye) = posed.to_scale_rotation_translation();
    let fov = if node.camera_fov > 0.0 {
        node.camera_fov
    } else {
        DIALOG_FOV
    };
    Some((
        eye.to_array(),
        angles_of(turn * Vec3::NEG_Z, turn * Vec3::Y),
        fov,
    ))
}

/// The Quake angles (pitch, yaw, roll) of a camera looking along `forward`
/// with `up` its top: a roll turns the level camera's up toward its right.
fn angles_of(forward: Vec3, up: Vec3) -> [f32; 3] {
    let mut angles = angles::look_angles(forward.to_array());
    let (_, right, level_up) = sim::math::angle_vectors(angles);
    angles[2] = up
        .dot(Vec3::from_array(right))
        .atan2(up.dot(Vec3::from_array(level_up)))
        .to_degrees();
    angles
}

/// Node `at` of `model` in the model's space as `clip` poses it at
/// `seconds`, its position keys times `scale` (`asset_odyssey::anim::posed`).
fn posed_node(
    model: &ModelIr,
    (clip, scale): (&AnimationIr, f32),
    at: usize,
    seconds: f32,
) -> Mat4 {
    let bound = clip.bind(model);
    crate::scene::to_mat4(&asset_odyssey::anim::posed(
        model,
        (clip, &bound),
        scale,
        at,
        seconds,
    ))
}

/// Puts the participants of the staged conversation `seat`'s screen shows
/// on their marks (reone's stunt mode): for each, its line's clip (its
/// `AnimList` entry, else the camera's) of its stunt model, at the time
/// since the line showed, held at the clip's end, played at scale 1 as
/// reone plays stunts. A body whose nodes the clip names stands with its
/// root where the stunt model's `cutscenedummy` stands, and the clip poses
/// its limbs; any other body (JA's own model) is turned and moved there as
/// drawn, a KOTOR model facing its +Y. A placeable (a cutscene's screens and
/// ships) is its model as the scene placed it, posed the same way. A
/// character's clip's own events passed since the last frame are heard as
/// a script's clip's are (`clip_events`), returned.
fn stage_stunts(
    view: &View,
    ms: f64,
    scene: &crate::viewer::LoadedScene,
    transforms: &mut Query<&mut Transform>,
) -> Vec<(SimId, crate::clip_events::Heard)> {
    let mut heard = Vec::new();
    let Some(latest) = view.history.last() else {
        return heard;
    };
    let Some(talk) = talk_shown(view, view.cast.seat) else {
        return heard;
    };
    let Some(dlg) = view
        .level
        .scripts()
        .and_then(|scripts| scripts.dialogue(&talk.dialog))
    else {
        return heard;
    };
    let Some(line) = dlg
        .entries
        .get(talk.entry as usize)
        .filter(|_| dlg.staged())
    else {
        return heard;
    };
    let objects = Instances::in_snapshot(&view.level, latest);
    let seconds = ((ms - talk.shown_ms as f64) / 1000.0).max(0.0) as f32;
    let shown = (talk.dialog.as_str(), talk.entry, talk.shown_ms);
    for (tag, stunt) in &dlg.stunts {
        let id = sim::script::dialogue::participant(
            tag,
            (SimId(talk.owner.0), SimId(talk.pc.0)),
            |tag| cast::tagged(latest, &objects, tag),
        );
        let clip_of = |stunt: String, name: String, model: &ModelIr| {
            view.stunt_clips
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry((stunt, name.clone()))
                .or_insert_with(|| model.animation(&name).cloned().map(Arc::new))
                .clone()
        };
        let Some(StuntMark {
            clip,
            at_clip,
            place,
            turn,
        }) = stunt_mark(&scene.cutscene_models, line, (tag, stunt), clip_of, seconds)
        else {
            continue;
        };
        let mark = (place, turn, &clip, at_clip);
        let body = id.and_then(|id| {
            Some((
                view.cast.drawn_body(id)?,
                cast::blended(&view.history, id, ms)?,
            ))
        });
        if let Some(id) = id {
            let from = view.stunts_heard.from(tag, shown, (ms, at_clip));
            heard.extend(
                crate::model_fx::passed(&clip, from, at_clip)
                    .filter_map(crate::clip_events::Heard::of)
                    .map(|event| (id, event)),
            );
        }
        let Some((body, at)) = body else {
            // Not a character: a placeable the scene drew.
            let placeable = objects
                .placements()
                .find(|(_, o)| o.tag.eq_ignore_ascii_case(tag))
                .and_then(|(number, _)| scene.instance_models.get(&number))
                .and_then(|&index| {
                    let (root, nodes) = view.models.get(index)?.as_ref()?;
                    Some((scene.models.get(index)?, *root, nodes.as_slice()))
                });
            if let Some((placed, root, nodes)) = placeable
                && !on_mark((placed, root, nodes), mark, transforms)
                && let Ok(mut t) = transforms.get_mut(root)
            {
                *t = Transform::from_translation(place).with_rotation(turn);
            }
            continue;
        };
        let staged = body.kotor.is_some_and(|(index, nodes)| {
            let head = crate::face::hung_head(&scene.models, index).and_then(|head| {
                let (_, nodes) = view.models.get(head)?.as_ref()?;
                Some((scene.models.get(head)?, nodes.as_slice()))
            });
            scene.models.get(index).is_some_and(|placed| {
                let staged = on_mark((placed, body.root, nodes), mark, transforms);
                if staged && let Some(head) = head {
                    face_on_mark(head, mark, transforms);
                }
                staged
            })
        });
        match (staged, body.yaw) {
            (true, _) | (false, None) => {}
            (false, Some(yaw)) => {
                let soles = cast::soles(at.origin);
                let facing = turn * Vec3::Y;
                let delta = Quat::from_rotation_z(facing.y.atan2(facing.x) - yaw.to_radians());
                if let Ok(mut t) = transforms.get_mut(body.root) {
                    t.translation = place + delta * (t.translation - soles);
                    t.rotation = delta * t.rotation;
                }
            }
        }
    }
    heard
}

/// Where a staged line's stunt stands: its clip, the time into it, and its
/// `cutscenedummy`'s place and turn.
struct StuntMark {
    clip: Arc<AnimationIr>,
    at_clip: f32,
    place: Vec3,
    turn: Quat,
}

/// The mark of participant `tag`'s stunt (`stunt`, a model of `models`) for
/// `line`, `seconds` after it showed: its line's clip (its `AnimList` entry,
/// else the camera's, by `clip_of`: the stunt's name, the clip's, its
/// model), held at its end, played at scale 1 as reone plays stunts.
fn stunt_mark(
    models: &BTreeMap<String, Arc<ModelIr>>,
    line: &fmt_odyssey::dlg::Node,
    (tag, stunt): (&str, &str),
    clip_of: impl FnOnce(String, String, &ModelIr) -> Option<Arc<AnimationIr>>,
    seconds: f32,
) -> Option<StuntMark> {
    let animation = line
        .anims
        .iter()
        .find(|(who, animation)| {
            who.eq_ignore_ascii_case(tag) && sim::script::names_clip(*animation)
        })
        .map_or(line.camera_animation, |(_, animation)| *animation);
    let stunt = stunt.to_ascii_lowercase();
    let model = models.get(&stunt)?;
    let name = sim::script::camera_clip_name(animation)?;
    let clip = clip_of(stunt, name, model)?;
    let dummy = model.node_index(hooks::CUTSCENE_DUMMY)?;
    let at_clip = seconds.min(clip.length);
    let (_, turn, place) =
        posed_node(model, (&clip, 1.0), dummy, at_clip).to_scale_rotation_translation();
    Some(StuntMark {
        clip,
        at_clip,
        place,
        turn,
    })
}

/// Where the view draws the participants of `talk`'s staged line at `ms`
/// (`stage_stunts`): each one the scene names, by its stunt model of
/// `models`, on its mark (game space, metres; its soles). None where the
/// line is not staged.
pub fn staged_places(
    level: &sim::Level,
    snapshot: &Snapshot,
    talk: &sim::script::dialogue::Conversation,
    models: &BTreeMap<String, Arc<ModelIr>>,
    ms: f64,
) -> Vec<(SimId, Vec3)> {
    let Some(dlg) = level
        .scripts()
        .and_then(|scripts| scripts.dialogue(&talk.dialog))
    else {
        return Vec::new();
    };
    let Some(line) = dlg
        .entries
        .get(talk.entry as usize)
        .filter(|_| dlg.staged())
    else {
        return Vec::new();
    };
    let objects = Instances::in_snapshot(level, snapshot);
    let seconds = ((ms - talk.shown_ms as f64) / 1000.0).max(0.0) as f32;
    let clip_of = |_, name: String, model: &ModelIr| model.animation(&name).cloned().map(Arc::new);
    dlg.stunts
        .iter()
        .filter_map(|(tag, stunt)| {
            let id = sim::script::dialogue::participant(
                tag,
                (SimId(talk.owner.0), SimId(talk.pc.0)),
                |tag| cast::tagged(snapshot, &objects, tag),
            )?;
            let mark = stunt_mark(models, line, (tag, stunt), clip_of, seconds)?;
            Some((id, mark.place))
        })
        .collect()
}

/// The stunts' hearing probes (`kjk sim probes --only "stunt events"`): a line's clip is
/// heard from its start, then from where it was last heard; a line under
/// way when stunts were first staged, from where it is.
#[cfg(feature = "probes")]
pub fn stunt_heard_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let heard = StuntsHeard::default();
    let late = heard.from("carth", ("m01aa_c01", 3, 500), (1000.0, 0.5));
    let next = heard.from("carth", ("m01aa_c01", 4, 2000), (2100.0, 0.1));
    let on = heard.from("carth", ("m01aa_c01", 4, 2000), (2200.0, 0.2));
    let other = heard.from("trask", ("m01aa_c01", 4, 2000), (2200.0, 0.2));
    vec![(
        "a stunt clip heard from where it was last heard",
        check(
            late == 0.5 && next == 0.0 && on == 0.1 && other == 0.0,
            format!("under way {late}, next line {next}, then {on}, another {other}"),
        ),
    )]
}

/// A staged line as shown: its conversation, entry and when it showed.
type ShownLine<'a> = (&'a str, u32, u64);

/// How far into its stunt clip each participant (by tag) was heard, and of
/// which line; and when (view ms) stunts were first staged.
#[derive(Default)]
struct StuntsHeard(std::sync::Mutex<HeardSoFar>);

#[derive(Default)]
struct HeardSoFar {
    at: BTreeMap<String, ((String, u32, u64), f32)>,
    first: Option<f64>,
}

impl StuntsHeard {
    /// Where `tag`'s clip is heard from, now at `at` seconds into it at
    /// `ms` (view ms) of line `shown`: where it was last heard, else from
    /// the start of a line shown since stunts were first staged (a line
    /// already under way then, a save loaded mid-line, from now).
    fn from(&self, tag: &str, (dialog, entry, since): ShownLine, (ms, at): (f64, f32)) -> f32 {
        let mut heard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = *heard.first.get_or_insert(ms);
        let line = ((dialog.to_string(), entry, since), at);
        let was = heard.at.insert(tag.to_ascii_lowercase(), line);
        match was {
            Some(((d, e, s), was)) if d == dialog && e == entry && s == since => was.min(at),
            _ if since as f64 >= first => 0.0,
            _ => at,
        }
    }
}

/// A drawn model (`placed`, its root and node entities) on a stunt mark:
/// its root at `place` turned by `turn`, and `clip` at `at` seconds on the
/// nodes it names, at scale 1; its own `cutscenedummy` left at rest (the
/// root stands on the mark already). False, and nothing moved, when the
/// clip names none of its nodes.
fn on_mark(
    (placed, root, nodes): (&crate::viewer::Placed, Entity, &[Entity]),
    (place, turn, clip, at): (Vec3, Quat, &Arc<AnimationIr>, f32),
    transforms: &mut Query<&mut Transform>,
) -> bool {
    let Some(animator) = crate::scene::animator(placed, clip, nodes) else {
        return false;
    };
    if let Ok(mut t) = transforms.get_mut(root) {
        *t = Transform::from_translation(place).with_rotation(turn);
    }
    animator.at_scale(1.0).pose(at, transforms);
    let own_dummy = placed
        .ir
        .node_index(hooks::CUTSCENE_DUMMY)
        .and_then(|n| Some((*nodes.get(n)?, &placed.ir.nodes[n])));
    if let Some((entity, rest)) = own_dummy
        && let Ok(mut t) = transforms.get_mut(entity)
    {
        let [x, y, z, w] = rest.rotation;
        *t = Transform::from_translation(Vec3::from_array(rest.translation))
            .with_rotation(Quat::from_xyzw(x, y, z, w));
    }
    true
}

/// The stunt clip a body plays on its mark (`on_mark`), on the face of the
/// head hung on it (`face`): its eyelids shut while the stunt sleeps.
fn face_on_mark(
    (head, nodes): (&crate::viewer::Placed, &[Entity]),
    (_, _, clip, at): (Vec3, Quat, &Arc<AnimationIr>, f32),
    transforms: &mut Query<&mut Transform>,
) {
    if let Some(animator) = crate::scene::animator(head, clip, nodes) {
        animator.at_scale(1.0).pose(at, transforms);
    }
}

/// A placed camera's shot: at its position raised by its height, turned by
/// its orientation after a tip about its own x axis by its pitch (0 looks
/// straight down, 90 level along its y). Its field of view is the
/// vertical one; none set keeps the play camera's.
fn placed_shot(camera: &PlacedCamera) -> TalkShot {
    let [w, x, y, z] = camera.orientation;
    let pitch = camera.pitch.to_radians();
    let forward =
        Quat::from_xyzw(x, y, z, w).normalize() * Vec3::new(0.0, pitch.sin(), -pitch.cos());
    let eye = Vec3::from_array(camera.position) + Vec3::Z * camera.height;
    let fov = if camera.field_of_view > 0.0 {
        camera.field_of_view
    } else {
        FOV_DEGREES
    };
    (eye.to_array(), angles::look_angles(forward.to_array()), fov)
}

/// The camera where the view has it: a conversation's `talk` shot while
/// the player is in one, else its mode's (`first`, the eye and view
/// angles, for `First`), unless a `camera` command took it.
fn place_camera(
    view: &View,
    (talk, first): (Option<TalkShot>, Option<EyeShot>),
    (soles, fov, shaken): (Vec3, f32, [f32; 2]),
    transforms: &mut Query<&mut Transform>,
    cameras: &mut Query<(Entity, &mut Projection), SceneCamera>,
) -> Option<sim::target::Look> {
    if view.fixed {
        return None;
    }
    let mut aim = None;
    let (location, angles, fov) = match (talk, first, view.camera_mode) {
        (Some(talking), ..) => talking,
        (None, Some((eye, angles)), _) => {
            aim = Some(sim::target::Look {
                from: eye,
                forward: sim::math::angle_vectors(angles).0,
            });
            (eye, angles, fov)
        }
        (None, None, CameraMode::Third | CameraMode::First) => {
            let (t, l) = (view.camera.target, view.camera.location);
            let mut look: [f32; 3] = std::array::from_fn(|i| t[i] - l[i]);
            let from = l.map(|x| x * UNIT);
            let angles = angles::look_angles(look);
            if sim::math::normalize(&mut look) > 0.0 {
                aim = Some(sim::target::Look {
                    from,
                    forward: look,
                });
            }
            (from, angles, fov)
        }
        (None, None, CameraMode::Side(distance)) => (
            [soles.x + distance, soles.y, soles.z + 1.5],
            [14.0, 180.0, 0.0],
            fov,
        ),
        (None, None, CameraMode::Arc(distance)) => {
            ([soles.x + distance, soles.y, 1.1], [4.0, 180.0, 0.0], fov)
        }
    };
    // A screen shake turns the camera, not where it aims.
    let angles = [angles[0] + shaken[0], angles[1] + shaken[1], angles[2]];
    let rotation = game_to_bevy();
    for (entity, mut projection) in cameras.iter_mut() {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let (forward, _, up) = sim::math::angle_vectors(angles);
            *transform = Transform::from_translation(rotation * Vec3::from_array(location))
                .looking_to(
                    rotation * Vec3::from_array(forward),
                    rotation * Vec3::from_array(up),
                );
        }
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = fov.to_radians();
        }
    }
    aim
}
