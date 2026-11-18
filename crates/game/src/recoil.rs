//! A character firing a blaster feels the shot in its gun, its arms and its
//! body.
//!
//! - **The gun:** each shot pushes the gun back along its barrel and lifts
//!   its muzzle at the wrist, as far as the blaster's handling says
//!   (`push`, `weapons::handling`). The arm's solver then reaches the gun
//!   where the shot put it (`asset_ja::rig::Aim`). The elbow bends to take
//!   the push, and a rifle's support hand stays on the gun.
//! - **The shoulder:** the same push turns the gun-side collar bone back
//!   about the up axis before the arm reaches the gun
//!   (`Push::shoulder_deg`), so the shoulder gives way a little. A rifle's
//!   shoulder turns 6° and a pistol's 2°, at a scale of 1.
//! - **The body:** after the arms are posed, a rifle shot turns the chest
//!   2.5° about the up axis, so the gun side goes back with the shoulder
//!   (`turns`). The chest carries both arms and the gun, so the support
//!   hand stays where it holds the gun. A pistol's forearm and upper arm
//!   follow the wrist's kick up by 2° and 1°.
//! - **How much:** the shoulder's and the body's turns are scaled by the
//!   gun's own kick rule against the wield's reference gun (`scale`). Every
//!   gun that is not a pistol moves like a rifle.
//! - **How fast:** the gun's push rises and settles as its handling says.
//!   The shoulder's and the body's turns rise over `RISE_MS` and settle to
//!   exactly nothing over the wield's settle time (`share`). A new shot
//!   rises from what the last one left, so rapid fire never passes the
//!   motion of a single shot.
//! - **A pair** moves only the side whose gun fired.
//! - **Sustained fire:** from the third shot of a streak (`Streak`), the
//!   chest leans a few degrees toward the target (`lean_deg`). It holds
//!   the lean while the streak lasts and straightens once it ends.
//! - **First person:** the body looked through keeps only the gun's push.
//!   Its gun moves back along the line of sight and its muzzle lifts at the
//!   wrist. No bone of that body turns, and its chest does not lean.
//!
//! The view alone: the sim's skeleton and its aim are not touched.

use bevy::math::{Quat, Vec3};
use sim::combat::sheet::Blaster;

use crate::aim_pose::Wield;

/// How long a shot's motion takes to reach its full size (ms).
pub(crate) const RISE_MS: f64 = 50.0;
/// How long a pistol's motion takes to settle after its full size (ms).
pub(crate) const PISTOL_SETTLE_MS: f64 = 250.0;
/// How long a rifle's motion takes to settle after its full size (ms).
pub(crate) const RIFLE_SETTLE_MS: f64 = 300.0;
/// The smallest scale of a wield's motion, for a gun that barely kicks.
pub(crate) const MIN_SCALE: f32 = 0.5;
/// The largest scale of a wield's motion, for a gun that kicks hard.
pub(crate) const MAX_SCALE: f32 = 2.0;
/// The forearm's outward turn (degrees) for each degree of the gun's
/// sideways kick.
pub(crate) const SIDE_PER_DEG: f32 = 4.0;
/// How far a pistol shot turns the collar bone back (degrees, at a scale
/// of 1).
pub(crate) const PISTOL_SHOULDER_DEG: f32 = 2.0;
/// How far a rifle shot turns the collar bone back (degrees, at a scale of
/// 1).
pub(crate) const RIFLE_SHOULDER_DEG: f32 = 6.0;

/// The chest bone.
const CHEST: &str = "torsoupr_g";

/// How hard a gun kicks the aim (degrees): up, and to one side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Kick {
    pub up: f32,
    pub side: f32,
}

/// The kick of `blaster`, as its rule says (`sim::combat::bloom::tune`).
pub(crate) fn kick_of(blaster: Blaster) -> Kick {
    let tune = sim::combat::bloom::tune(blaster);
    Kick {
        up: tune.kick_up,
        side: tune.kick_side,
    }
}

/// Each hand's last shot and the one before it (sim ms). The first of each
/// pair is the right hand, the second the left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shots {
    pub last: [Option<f64>; 2],
    pub before: [Option<f64>; 2],
}

impl Shots {
    /// The shots of the guns held, from each hand's `last` shot and the one
    /// `before` it. A pair keeps each hand's own. One gun is the gun out
    /// whichever hand the sim names, so it takes the shots of the hand that
    /// fired last, in the first slot.
    pub(crate) fn held(last: [Option<f64>; 2], before: [Option<f64>; 2], pair: bool) -> Shots {
        if pair {
            return Shots { last, before };
        }
        let hand =
            usize::from(last[1].is_some_and(|left| last[0].is_none_or(|right| left > right)));
        Shots {
            last: [last[hand], None],
            before: [before[hand], None],
        }
    }

    /// Hand `hand`'s last shot and the time since the shot before it (ms).
    fn of(&self, hand: usize) -> Option<(f64, Option<f64>)> {
        let last = self.last[hand]?;
        Some((last, self.before[hand].map(|before| last - before)))
    }
}

/// The longest gap between two shots of one streak (ms). It is longer than
/// the 1000 ms between two shots of the slowest gun.
pub(crate) const SUSTAIN_GAP_MS: f64 = 1100.0;
/// The shot of a streak that starts the chest's lean.
pub(crate) const LEAN_SHOTS: u16 = 3;
/// How far the chest leans toward the aim during sustained fire (degrees).
pub(crate) const LEAN_DEG: f32 = 4.0;
/// How long the chest takes to lean in fully (ms).
pub(crate) const LEAN_IN_MS: f64 = 300.0;
/// How long the chest takes to straighten once the streak ends (ms).
pub(crate) const LEAN_OUT_MS: f64 = 400.0;

/// A run of shots with no gap longer than `SUSTAIN_GAP_MS`. It keeps when
/// the run began, how many shots it has, when its third shot came (the
/// start of the lean), and its last shot (sim ms). It also keeps the run
/// before it while that run's lean still eases out (`ended`), so a new run
/// never cuts that lean short.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Streak {
    since_ms: f64,
    shots: u16,
    leaned_ms: Option<f64>,
    last_ms: f64,
    /// The run before: when it leaned and its last shot.
    ended: Option<(f64, f64)>,
}

impl Streak {
    /// Counts a shot fired at `at_ms`. A shot after a gap longer than
    /// `SUSTAIN_GAP_MS` starts a new run.
    pub(crate) fn shot(&mut self, at_ms: f64) {
        if self.shots == 0 || at_ms - self.last_ms > SUSTAIN_GAP_MS {
            *self = Streak {
                since_ms: at_ms,
                shots: 0,
                leaned_ms: None,
                last_ms: at_ms,
                ended: self.leaned_ms.map(|leaned| (leaned, self.last_ms)),
            };
        }
        self.shots = self.shots.saturating_add(1);
        if self.shots == LEAN_SHOTS {
            self.leaned_ms = Some(at_ms);
        }
        self.last_ms = at_ms;
    }
}

/// The chest's lean toward the aim at `now_ms` (degrees, 0 to `LEAN_DEG`).
/// It eases in over `LEAN_IN_MS` from the streak's third shot, holds until
/// `SUSTAIN_GAP_MS` after its last shot, and eases out over `LEAN_OUT_MS`.
/// While the run before still eases out, the larger of the two leans
/// shows.
pub(crate) fn lean_deg(streak: &Streak, now_ms: f64) -> f32 {
    let current = streak.leaned_ms.map(|leaned| (leaned, streak.last_ms));
    let lean = [current, streak.ended]
        .into_iter()
        .flatten()
        .map(|(leaned_ms, last_ms)| {
            let end = last_ms + SUSTAIN_GAP_MS;
            smoothstep((now_ms.min(end) - leaned_ms) / LEAN_IN_MS)
                * (1.0 - smoothstep((now_ms - end) / LEAN_OUT_MS))
        })
        .fold(0.0, f64::max);
    LEAN_DEG * lean as f32
}

/// What the motion needs of one body this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Firing {
    /// How the body holds its guns, or `None` without a gun in hand.
    pub wield: Option<Wield>,
    /// The blaster in each hand, the right first. A pair has one in each
    /// hand, one gun has it first, and a hand without a gun has none.
    pub blasters: [Option<Blaster>; 2],
    pub shots: Shots,
    pub streak: Streak,
    /// The player looks through this body's eyes (first person).
    pub looked_through: bool,
    pub alive: bool,
}

/// A shot's push on one gun at one moment. `up_deg` lifts the muzzle about
/// the wrist, `back_m` moves the gun back along its barrel (metres), and
/// `shoulder_deg` turns the gun-side collar bone back about the up axis
/// before the arm reaches the gun.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Push {
    pub up_deg: f32,
    pub back_m: f32,
    pub shoulder_deg: f32,
}

/// A turn over a posed bone. Its pitch turns the bone about the body's
/// right axis, and a positive pitch lifts the muzzle or rocks the bone
/// back. Its yaw turns the bone about the body's up axis. A positive yaw
/// turns an arm bone outward, and turns the chest so its right side goes
/// back. Both are in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Turn {
    pub node: &'static str,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
}

/// A bone a shot turns. `nodes` holds the bone when the right hand fires
/// and when the left one does. `pitch_deg` and `yaw_deg` are its turn at a
/// scale of 1. The forearm also turns outward with the gun's sideways kick
/// (`sideways`).
struct Part {
    nodes: [&'static str; 2],
    pitch_deg: f32,
    yaw_deg: f32,
    sideways: bool,
}

/// A pistol, or each gun of a pair: the forearm and the upper arm follow
/// the wrist's kick up.
const PISTOL: &[Part] = &[
    Part {
        nodes: ["rforearm_g", "lforearm_g"],
        pitch_deg: 2.0,
        yaw_deg: 0.0,
        sideways: true,
    },
    Part {
        nodes: ["rbicep_g", "lbicep_g"],
        pitch_deg: 1.0,
        yaw_deg: 0.0,
        sideways: false,
    },
];
/// A rifle: the chest turns so the gun side goes back with the shoulder.
/// It turns both arms and the gun with it.
const RIFLE: &[Part] = &[Part {
    nodes: [CHEST, CHEST],
    pitch_deg: 0.0,
    yaw_deg: 2.5,
    sideways: false,
}];

fn parts(wield: Wield) -> &'static [Part] {
    match wield {
        Wield::Pistol | Wield::Pair => PISTOL,
        Wield::Rifle => RIFLE,
    }
}

/// How far the wield's shot turns the collar bone back (degrees, at a
/// scale of 1).
fn shoulder_of(wield: Wield) -> f32 {
    match wield {
        Wield::Pistol | Wield::Pair => PISTOL_SHOULDER_DEG,
        Wield::Rifle => RIFLE_SHOULDER_DEG,
    }
}

/// How long the wield's motion takes to settle after its full size (ms).
fn settle_ms(wield: Wield) -> f64 {
    match wield {
        Wield::Pistol | Wield::Pair => PISTOL_SETTLE_MS,
        Wield::Rifle => RIFLE_SETTLE_MS,
    }
}

/// `x` eased in and out, from 0 at 0 to 1 at 1.
fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A lone shot's share of its motion `t_ms` after it.
fn pulse(wield: Wield, t_ms: f64) -> f64 {
    if t_ms < 0.0 {
        0.0
    } else if t_ms < RISE_MS {
        smoothstep(t_ms / RISE_MS)
    } else {
        1.0 - smoothstep((t_ms - RISE_MS) / settle_ms(wield))
    }
}

/// The share of a shot's motion (0 to 1) `since_ms` after it, `gap_ms`
/// after the shot before it. While it rises, it starts from what the shot
/// before left, and it never falls below what that shot still shows.
pub(crate) fn share(wield: Wield, since_ms: f64, gap_ms: Option<f64>) -> f32 {
    let share = if since_ms < 0.0 {
        0.0
    } else if since_ms < RISE_MS {
        let rise = smoothstep(since_ms / RISE_MS);
        gap_ms.map_or(rise, |gap| {
            let left = pulse(wield, gap);
            (left + (1.0 - left) * rise).max(pulse(wield, since_ms + gap))
        })
    } else {
        pulse(wield, since_ms)
    };
    share as f32
}

/// The scale of the wield's motion for a gun of `kick`: its upward kick
/// against the wield's reference gun, the blaster pistol for a pistol or a
/// pair and the blaster rifle for any other gun.
pub(crate) fn scale(wield: Wield, kick: Kick) -> f32 {
    let reference = match wield {
        Wield::Pistol | Wield::Pair => rules::weapons::pistol::KICK[0],
        Wield::Rifle => rules::weapons::rifle::KICK[0],
    };
    (kick.up / reference).clamp(MIN_SCALE, MAX_SCALE)
}

/// Adds a turn of `node`, to any it already has.
fn add(turns: &mut Vec<Turn>, node: &'static str, pitch_deg: f32, yaw_deg: f32) {
    if pitch_deg == 0.0 && yaw_deg == 0.0 {
        return;
    }
    match turns.iter_mut().find(|turn| turn.node == node) {
        Some(turn) => {
            turn.pitch_deg += pitch_deg;
            turn.yaw_deg += yaw_deg;
        }
        None => turns.push(Turn {
            node,
            pitch_deg,
            yaw_deg,
        }),
    }
}

/// The push of hand `hand`'s gun at `now_ms` (the first hand is the right,
/// or one gun). A body that is dead, or whose hand holds no gun, has none.
/// The gun's push follows its handling (`weapons::Handling::kick`). The
/// shoulder's turn follows the wield's motion, and the body looked through
/// has none.
pub(crate) fn push(firing: &Firing, hand: usize, now_ms: f64) -> Push {
    let (Some(wield), Some(blaster), Some((last, gap))) = (
        firing.wield.filter(|_| firing.alive),
        firing.blasters[hand],
        firing.shots.of(hand),
    ) else {
        return Push::default();
    };
    let handling = crate::weapons::handling(blaster);
    let gun = handling.kick((now_ms - last) as f32, gap.map(|gap| gap as f32));
    let shoulder = if firing.looked_through {
        0.0
    } else {
        shoulder_of(wield) * scale(wield, kick_of(blaster)) * share(wield, now_ms - last, gap)
    };
    Push {
        up_deg: handling.kick_degrees * gun,
        back_m: handling.kick_back * gun,
        shoulder_deg: shoulder,
    }
}

/// Every turn of the body's bones at `now_ms`, added after the arms are
/// posed. A body that is dead, has no gun in hand, or is looked through in
/// first person has none. A pair moves each side by its own hand's shots,
/// and one gun moves by the shots in the first slot (`Shots::held`).
/// During sustained fire the chest also leans forward (`lean_deg`).
pub(crate) fn turns(firing: &Firing, now_ms: f64) -> Vec<Turn> {
    let mut turns = Vec::new();
    let Some(wield) = firing
        .wield
        .filter(|_| firing.alive && !firing.looked_through)
    else {
        return turns;
    };
    let hands: &[usize] = match wield {
        Wield::Pair => &[0, 1],
        _ => &[0],
    };
    for &hand in hands {
        let (Some((last, gap)), Some(blaster)) = (firing.shots.of(hand), firing.blasters[hand])
        else {
            continue;
        };
        let kick = kick_of(blaster);
        let (scale, share) = (scale(wield, kick), share(wield, now_ms - last, gap));
        for part in parts(wield) {
            let sideways = if part.sideways {
                kick.side * SIDE_PER_DEG
            } else {
                0.0
            };
            add(
                &mut turns,
                part.nodes[hand],
                part.pitch_deg * scale * share,
                (part.yaw_deg * scale + sideways) * share,
            );
        }
    }
    add(&mut turns, CHEST, -lean_deg(&firing.streak, now_ms), 0.0);
    turns
}

/// `turn` as a turn in the model's space, about the body's `right` and `up`
/// axes there. An arm bone of the right side and the chest turn the other
/// way about the up axis, so a positive yaw turns a right arm outward and
/// takes the chest's right side back.
pub(crate) fn rotation(turn: &Turn, right: Vec3, up: Vec3) -> Quat {
    let outward = if turn.node.starts_with('l') {
        1.0
    } else {
        -1.0
    };
    Quat::from_axis_angle(up, outward * turn.yaw_deg.to_radians())
        * Quat::from_axis_angle(right, turn.pitch_deg.to_radians())
}

/// The probes (`kjk sim probes --only "firing motion"`). The checks on a
/// posed KOTOR body (`posed`) need JA and KOTOR installed.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    let mut checks = checks::all();
    checks.extend(posed::checks());
    checks
}

#[cfg(feature = "probes")]
mod posed;

#[cfg(feature = "probes")]
mod checks {
    use std::collections::BTreeMap;

    use sim::probe::check;

    use super::*;

    /// How often the probes sample the motion (ms), about one frame.
    const STEP_MS: f64 = 8.0;
    const WIELDS: [Wield; 3] = [Wield::Pistol, Wield::Pair, Wield::Rifle];
    const BLASTERS: [Blaster; 7] = [
        Blaster::Pistol,
        Blaster::Rifle,
        Blaster::Repeater,
        Blaster::Bowcaster,
        Blaster::Disruptor,
        Blaster::Ion,
        Blaster::Sonic,
    ];
    /// The collar bones, the right first. The probes show each hand's
    /// shoulder push (`Push::shoulder_deg`) as a yaw of its collar bone.
    const COLLARS: [&str; 2] = ["rcollar_g", "lcollar_g"];
    /// The most one step may move a gun back (metres) or lift its muzzle
    /// (degrees).
    const PUSH_STEP: (f32, f32) = (0.025, 1.0);
    /// The share of its peak a gun's push may still show 1500 ms after a
    /// lone shot.
    const PUSH_LEFT: f32 = 0.02;

    /// A body firing. Each shot is its time (sim ms) and its hand (0 the
    /// right, 1 the left), in the order fired. A body knows only the shots
    /// fired by the time it is drawn, unless `ahead` lets it see them all.
    /// Its shots count toward a streak only when it `leans`, so the probes
    /// of the arms' motion see that motion alone.
    struct Run {
        wield: Wield,
        blaster: Blaster,
        shots: Vec<(f64, usize)>,
        ahead: bool,
        looked_through: bool,
        leans: bool,
    }

    impl Run {
        fn new(wield: Wield, blaster: Blaster, shots: Vec<(f64, usize)>) -> Self {
            Run {
                wield,
                blaster,
                shots,
                ahead: false,
                looked_through: false,
                leans: false,
            }
        }

        /// A rifle fired at each of `times`, its shots counted in a streak.
        fn streak(times: impl IntoIterator<Item = f64>) -> Self {
            let shots = times.into_iter().map(|at| (at, 0)).collect();
            Run {
                leans: true,
                ..Run::new(Wield::Rifle, Blaster::Rifle, shots)
            }
        }

        /// The chest's lean at `now_ms`.
        fn lean(&self, now_ms: f64) -> f32 {
            lean_deg(&self.firing(now_ms).streak, now_ms)
        }

        /// Every hand of the wield fires once at `at`, a shot the body
        /// knows of even before it comes.
        fn single(wield: Wield, blaster: Blaster, at: f64) -> Self {
            let shots = hands(wield).iter().map(|&hand| (at, hand)).collect();
            Run {
                ahead: true,
                ..Run::new(wield, blaster, shots)
            }
        }

        /// One shot every `every_ms` from 0 until `until_ms`, the hands of a
        /// pair taking turns.
        fn steady(wield: Wield, blaster: Blaster, every_ms: f64, until_ms: f64) -> Self {
            let fired = hands(wield);
            let shots = (0..)
                .map(|n| (n as f64 * every_ms, fired[n % fired.len()]))
                .take_while(|(at, _)| *at < until_ms)
                .collect();
            Run::new(wield, blaster, shots)
        }

        fn firing(&self, now_ms: f64) -> Firing {
            let mut shots = Shots {
                last: [None; 2],
                before: [None; 2],
            };
            let mut streak = Streak::default();
            for &(at, hand) in self
                .shots
                .iter()
                .filter(|(at, _)| self.ahead || *at <= now_ms)
            {
                shots.before[hand] = shots.last[hand];
                shots.last[hand] = Some(at);
                if self.leans {
                    streak.shot(at);
                }
            }
            let blasters = match self.wield {
                Wield::Pair => [Some(self.blaster); 2],
                _ => [Some(self.blaster), None],
            };
            Firing {
                wield: Some(self.wield),
                blasters,
                shots,
                streak,
                looked_through: self.looked_through,
                alive: true,
            }
        }

        /// The turns at `now_ms`, with each hand's shoulder push as a yaw
        /// of its collar bone.
        fn at(&self, now_ms: f64) -> Vec<Turn> {
            let firing = self.firing(now_ms);
            let mut turns = turns(&firing, now_ms);
            for (hand, collar) in COLLARS.into_iter().enumerate() {
                add(
                    &mut turns,
                    collar,
                    0.0,
                    push(&firing, hand, now_ms).shoulder_deg,
                );
            }
            turns
        }

        /// Each hand's gun push at `now_ms`.
        fn pushes(&self, now_ms: f64) -> [Push; 2] {
            let firing = self.firing(now_ms);
            [0, 1].map(|hand| push(&firing, hand, now_ms))
        }

        /// The turns every `STEP_MS` from `from_ms` to `to_ms`.
        fn samples(&self, from_ms: f64, to_ms: f64) -> Vec<(f64, Vec<Turn>)> {
            times(from_ms, to_ms)
                .map(|now| (now, self.at(now)))
                .collect()
        }

        /// The span from 200 ms before its first shot until every motion
        /// has settled.
        fn span(&self) -> (f64, f64) {
            let first = self.shots.first().map_or(0.0, |shot| shot.0);
            let last = self.shots.last().map_or(0.0, |shot| shot.0);
            (
                first - 200.0,
                last + RISE_MS + settle_ms(self.wield) + 200.0,
            )
        }

        /// The samples over its whole span.
        fn whole(&self) -> Vec<(f64, Vec<Turn>)> {
            let (from, to) = self.span();
            self.samples(from, to)
        }

        fn label(&self) -> String {
            format!("{:?} with {:?}", self.wield, self.blaster)
        }
    }

    /// The hands that fire: both for a pair, else the right.
    fn hands(wield: Wield) -> &'static [usize] {
        match wield {
            Wield::Pair => &[0, 1],
            _ => &[0],
        }
    }

    /// The pitch and yaw of `node` (0 when it is not turned).
    fn angle(turns: &[Turn], node: &str) -> (f32, f32) {
        turns
            .iter()
            .find(|turn| turn.node == node)
            .map_or((0.0, 0.0), |turn| (turn.pitch_deg, turn.yaw_deg))
    }

    /// How far `node` is turned: its pitch and its yaw added.
    fn size(turns: &[Turn], node: &str) -> f32 {
        let (pitch, yaw) = angle(turns, node);
        pitch.abs() + yaw.abs()
    }

    /// Each bone's turn at the peak of a single shot from every hand.
    fn peaks(wield: Wield, blaster: Blaster) -> Vec<Turn> {
        Run::single(wield, blaster, 0.0).at(RISE_MS)
    }

    /// The largest pitch and yaw each bone reaches over `samples`.
    fn highest<'a>(
        samples: impl IntoIterator<Item = &'a (f64, Vec<Turn>)>,
    ) -> BTreeMap<&'static str, (f32, f32)> {
        let mut most = BTreeMap::new();
        for turn in samples.into_iter().flat_map(|(_, turns)| turns) {
            let (pitch, yaw) = most.entry(turn.node).or_insert((0.0f32, 0.0f32));
            *pitch = pitch.max(turn.pitch_deg);
            *yaw = yaw.max(turn.yaw_deg);
        }
        most
    }

    /// A single shot at 1000 ms of every wield with every gun.
    fn singles() -> Vec<Run> {
        WIELDS
            .iter()
            .flat_map(|&wield| BLASTERS.map(|blaster| Run::single(wield, blaster, 1000.0)))
            .collect()
    }

    /// A right-hand shot of a pair, at 0.
    fn pair_right() -> Run {
        Run::new(Wield::Pair, Blaster::Pistol, vec![(0.0, 0)])
    }

    /// A repeating blaster held as a rifle, firing every 100 ms for 5 s.
    fn rapid() -> Run {
        Run::steady(Wield::Rifle, Blaster::Repeater, 100.0, 5000.0)
    }

    /// Each wield firing every 40 ms for 5 s, as a fast creature can.
    fn fast() -> [Run; 3] {
        WIELDS.map(|wield| Run::steady(wield, own_gun(wield), 40.0, 5000.0))
    }

    /// Every run the hand-motion probes play.
    fn runs() -> Vec<Run> {
        let mut runs = singles();
        runs.extend([pair_right(), rapid()]);
        runs.extend(fast());
        runs
    }

    /// The gun a wield is usually seen with.
    fn own_gun(wield: Wield) -> Blaster {
        match wield {
            Wield::Pistol | Wield::Pair => Blaster::Pistol,
            Wield::Rifle => Blaster::Repeater,
        }
    }

    /// The first failure of `ok` over `cases`, or how many passed and what
    /// each one saw.
    fn every<T>(
        cases: impl IntoIterator<Item = T>,
        ok: impl Fn(&T) -> Result<String, String>,
    ) -> Result<String, String> {
        let mut seen = Vec::new();
        for case in cases {
            seen.push(ok(&case)?);
        }
        let count = seen.len();
        seen.retain(|what| !what.is_empty());
        if seen.is_empty() {
            Ok(format!("{count} cases"))
        } else {
            Ok(format!("{count} cases; {}", seen.join("; ")))
        }
    }

    /// A single shot's turns are 0 before it, rise to their peak over
    /// `RISE_MS`, and are exactly 0 from the end of the wield's settle time.
    fn settles(run: &Run) -> Result<String, String> {
        let shot = run.shots[0].0;
        let end = shot + RISE_MS + settle_ms(run.wield);
        let peak = run.at(shot + RISE_MS);
        if peak.is_empty() {
            return Err(format!("{}: no turn at the peak", run.label()));
        }
        for (now, turns) in run.samples(shot - 200.0, end + 200.0) {
            if (now < shot || now >= end) && !turns.is_empty() {
                return Err(format!(
                    "{}: turned at {now} ms (shot {shot}, settled {end})",
                    run.label()
                ));
            }
        }
        let rising = run.samples(shot, shot + RISE_MS);
        for turn in &peak {
            let sizes: Vec<f32> = rising
                .iter()
                .map(|(_, turns)| size(turns, turn.node))
                .collect();
            if !sizes.windows(2).all(|pair| pair[1] > pair[0]) {
                return Err(format!(
                    "{}: {} does not rise, {sizes:?}",
                    run.label(),
                    turn.node
                ));
            }
        }
        Ok(String::new())
    }

    /// No step of `run` moves a bone by more than a quarter of its
    /// single-shot peak, or by more than 3°.
    fn smooth(run: &Run) -> Result<String, String> {
        let peak = peaks(run.wield, run.blaster);
        let samples = run.whole();
        let mut largest = 0.0f32;
        for pair in samples.windows(2) {
            let ((_, was), (now, turns)) = (&pair[0], &pair[1]);
            for node in was.iter().chain(turns).map(|turn| turn.node) {
                let (top_pitch, top_yaw) = angle(&peak, node);
                let (pitch_was, yaw_was) = angle(was, node);
                let (pitch, yaw) = angle(turns, node);
                for (step, top, what) in [
                    ((pitch - pitch_was).abs(), top_pitch, "pitch"),
                    ((yaw - yaw_was).abs(), top_yaw, "yaw"),
                ] {
                    if step > 0.25 * top + 1e-5 || step > 3.0 {
                        return Err(format!(
                            "{}: {node}'s {what} moved {step:.3}° at {now} ms, peak {top:.3}°",
                            run.label()
                        ));
                    }
                    if top > 0.0 {
                        largest = largest.max(step / top);
                    }
                }
            }
        }
        Ok(format!(
            "{}: largest step {largest:.3} of the peak",
            run.label()
        ))
    }

    /// Sustained fire never passes the single-shot peak, and its peaks in
    /// the last second are within 5% of those in the first.
    fn steady_peaks(run: &Run) -> Result<String, String> {
        let peak = peaks(run.wield, run.blaster);
        let samples = run.whole();
        let most = highest(&samples);
        let second = |from: f64| {
            highest(
                samples
                    .iter()
                    .filter(|(now, _)| (from..from + 1000.0).contains(now)),
            )
        };
        let (first, last) = (second(0.0), second(4000.0));
        for (node, (pitch, yaw)) in &most {
            let (top_pitch, top_yaw) = angle(&peak, node);
            if *pitch > top_pitch + 1e-4 || *yaw > top_yaw + 1e-4 {
                return Err(format!(
                    "{}: {node} reached {pitch:.4}°/{yaw:.4}°, a single shot {top_pitch:.4}°/{top_yaw:.4}°",
                    run.label()
                ));
            }
            let size = |seen: &BTreeMap<&str, (f32, f32)>| {
                seen.get(node).map_or(0.0, |(pitch, yaw)| pitch + yaw)
            };
            let (early, late) = (size(&first), size(&last));
            if (late - early).abs() > 0.05 * early {
                return Err(format!(
                    "{}: {node} peaks at {early:.4}° in the first second, {late:.4}° in the last",
                    run.label()
                ));
            }
        }
        let shoulder =
            |seen: &BTreeMap<&str, (f32, f32)>| seen.get(COLLARS[0]).map_or(0.0, |p| p.1);
        Ok(format!(
            "{}: the shoulder peaks at {:.3}° in the first second and {:.3}° in the last, a single shot {:.3}°",
            run.label(),
            shoulder(&first),
            shoulder(&last),
            angle(&peak, COLLARS[0]).1
        ))
    }

    /// A lone shot's gun push: none before the shot; at its handling's
    /// peak, exactly the handling's kick up and back; never growing after
    /// that; and under `PUSH_LEFT` of its peak 1500 ms after the shot.
    fn push_settles(run: &Run) -> Result<String, String> {
        let shot = run.shots[0].0;
        let handling = crate::weapons::handling(run.blaster);
        let top = run.pushes(shot + f64::from(handling.rise_ms))[0];
        let early = run.pushes(shot - STEP_MS);
        let near = |got: f32, want: f32| (got - want).abs() <= 1e-4 * want.max(1.0);
        if !near(top.up_deg, handling.kick_degrees) || !near(top.back_m, handling.kick_back) {
            return Err(format!(
                "{}: the peak is {top:?}, the handling {}° and {} m",
                run.label(),
                handling.kick_degrees,
                handling.kick_back
            ));
        }
        if early.iter().any(|push| *push != Push::default()) {
            return Err(format!(
                "{}: pushed before the shot: {early:?}",
                run.label()
            ));
        }
        let after: Vec<f32> = times(shot + f64::from(handling.rise_ms), shot + 1500.0)
            .map(|now| run.pushes(now)[0].back_m)
            .collect();
        let falls = after.windows(2).all(|pair| pair[1] <= pair[0]);
        let left = after.last().copied().unwrap_or(0.0) / top.back_m;
        if !falls || left > PUSH_LEFT {
            return Err(format!(
                "{}: falls after its peak {falls}; {left:.4} of it left after 1500 ms",
                run.label()
            ));
        }
        Ok(String::new())
    }

    /// No step of `run` moves a gun back by more than `PUSH_STEP`'s metres
    /// or lifts its muzzle by more than its degrees, and the push never
    /// passes a single shot's.
    fn push_bounded(run: &Run) -> Result<String, String> {
        let handling = crate::weapons::handling(run.blaster);
        let (from, to) = run.span();
        let pushes: Vec<(f64, [Push; 2])> = times(from, to + 1500.0)
            .map(|now| (now, run.pushes(now)))
            .collect();
        let mut largest = (0.0f32, 0.0f32);
        for pair in pushes.windows(2) {
            let ((_, was), (now, pushed)) = (&pair[0], &pair[1]);
            for (was, push) in was.iter().zip(pushed) {
                let back = (push.back_m - was.back_m).abs();
                let up = (push.up_deg - was.up_deg).abs();
                largest = (largest.0.max(back), largest.1.max(up));
                if back > PUSH_STEP.0 || up > PUSH_STEP.1 {
                    return Err(format!(
                        "{}: the gun moved {back:.4} m back and {up:.3}° up at {now} ms",
                        run.label()
                    ));
                }
                if push.back_m > handling.kick_back + 1e-5 {
                    return Err(format!(
                        "{}: pushed {:.4} m at {now} ms, a single shot {:.4} m",
                        run.label(),
                        push.back_m,
                        handling.kick_back
                    ));
                }
            }
        }
        Ok(format!(
            "{}: largest step {:.4} m back, {:.3}° up",
            run.label(),
            largest.0,
            largest.1
        ))
    }

    pub(super) fn all() -> sim::probe::Checks {
        let singles = singles();
        vec![
            ("recoil_settles", every(&singles, |run| settles(run))),
            ("recoil_before_shot", {
                let early = singles.iter().find(|run| !run.at(990.0).is_empty());
                let below = singles.iter().find_map(|run| {
                    let samples = run.whole().into_iter();
                    let mut turns = samples
                        .flat_map(|(now, turns)| turns.into_iter().map(move |turn| (now, turn)));
                    let below = turns.find(|(_, turn)| turn.pitch_deg < 0.0 || turn.yaw_deg < 0.0);
                    below.map(|found| (run.label(), found))
                });
                check(
                    early.is_none() && below.is_none(),
                    format!(
                        "turned 10 ms before the shot: {:?}; below 0: {below:?}",
                        early.map(Run::label)
                    ),
                )
            }),
            ("recoil_kick_scales", {
                let shoulder = |blaster| angle(&peaks(Wield::Rifle, blaster), COLLARS[0]).1;
                let rifle = shoulder(Blaster::Rifle);
                let kick = |blaster| kick_of(blaster).up;
                let ion = shoulder(Blaster::Ion) / rifle;
                let disruptor = shoulder(Blaster::Disruptor) / rifle;
                let repeater = shoulder(Blaster::Repeater) / rifle;
                let near = |got: f32, want: f32| (got / want - 1.0).abs() <= 0.01;
                let mut by_kick = BLASTERS;
                by_kick.sort_by(|a, b| kick(*a).total_cmp(&kick(*b)));
                let rises = by_kick
                    .windows(2)
                    .all(|pair| shoulder(pair[1]) >= shoulder(pair[0]));
                check(
                    near(ion, kick(Blaster::Ion) / kick(Blaster::Rifle))
                        && near(disruptor, MAX_SCALE)
                        && near(repeater, kick(Blaster::Repeater) / kick(Blaster::Rifle))
                        && rises,
                    format!(
                        "the shoulder against the rifle's: ion {ion:.3}, disruptor {disruptor:.3}, repeater {repeater:.3}; never falls as the kick rises: {rises}"
                    ),
                )
            }),
            ("gun_push_settles", every(&singles, |run| push_settles(run))),
            ("gun_push_bounded", every(runs(), push_bounded)),
            ("pair_one_hand", {
                let run = pair_right();
                let moved: Vec<&str> = highest(&run.whole())
                    .into_iter()
                    .filter(|(node, _)| node.starts_with('l') || *node == CHEST)
                    .map(|(node, _)| node)
                    .collect();
                let (from, to) = run.span();
                let left = times(from, to).find(|&now| run.pushes(now)[1] != Push::default());
                check(
                    moved.is_empty() && left.is_none(),
                    format!("left side or chest turned: {moved:?}; left gun pushed at {left:?} ms"),
                )
            }),
            ("pair_both_hands", {
                let one = pair_right().at(RISE_MS);
                let both = peaks(Wield::Pair, Blaster::Pistol);
                let mirrored = one.iter().all(|turn| {
                    let left = format!("l{}", &turn.node[1..]);
                    angle(&both, turn.node) == (turn.pitch_deg, turn.yaw_deg)
                        && angle(&both, &left) == (turn.pitch_deg, turn.yaw_deg)
                });
                let chest = angle(&both, CHEST);
                check(
                    !one.is_empty() && mirrored && chest == (0.0, 0.0),
                    format!("one hand {one:?}; both {both:?}"),
                )
            }),
            ("hand_shots_one_gun", {
                let (last, before) = ([Some(100.0), Some(400.0)], [None, Some(50.0)]);
                let one = Shots::held(last, before, false);
                let pair = Shots::held(last, before, true);
                check(
                    one.last == [Some(400.0), None]
                        && one.before == [Some(50.0), None]
                        && pair.last == last
                        && pair.before == before,
                    format!("the left hand fired last: one gun {one:?}; a pair {pair:?}"),
                )
            }),
            ("recoil_rapid_fire", every([rapid()], steady_peaks)),
            ("recoil_fast_creature", every(fast(), steady_peaks)),
            ("recoil_no_jolt", every(runs(), smooth)),
            ("recoil_needs_gun", {
                let shot = Run::single(Wield::Rifle, Blaster::Rifle, 0.0).firing(20.0);
                let dead = Firing {
                    alive: false,
                    ..shot
                };
                let unarmed = Firing {
                    wield: None,
                    ..shot
                };
                let moved = |firing: &Firing| {
                    !turns(firing, 20.0).is_empty() || push(firing, 0, 20.0) != Push::default()
                };
                check(
                    !moved(&dead) && !moved(&unarmed),
                    format!(
                        "dead moves: {}; without a gun moves: {}",
                        moved(&dead),
                        moved(&unarmed)
                    ),
                )
            }),
            ("first_person_gun_only", {
                let firsts: Vec<Run> = WIELDS
                    .iter()
                    .map(|&wield| Run {
                        looked_through: true,
                        ..Run::single(wield, own_gun(wield), 0.0)
                    })
                    .chain([Run {
                        looked_through: true,
                        ..sustained()
                    }])
                    .collect();
                let turned = firsts.iter().find_map(|run| {
                    let (from, to) = run.span();
                    times(from, to)
                        .find(|&now| !run.at(now).is_empty())
                        .map(|now| (run.label(), now))
                });
                let unlike = firsts.iter().find_map(|first| {
                    let third = Run {
                        looked_through: false,
                        shots: first.shots.clone(),
                        ..Run::new(first.wield, first.blaster, Vec::new())
                    };
                    let (from, to) = first.span();
                    times(from, to)
                        .find(|&now| {
                            let (first, third) = (first.pushes(now), third.pushes(now));
                            (0..2).any(|hand| {
                                (first[hand].up_deg, first[hand].back_m)
                                    != (third[hand].up_deg, third[hand].back_m)
                            })
                        })
                        .map(|now| (first.label(), now))
                });
                let pushed = firsts.iter().any(|run| run.pushes(RISE_MS)[0].back_m > 0.0);
                check(
                    turned.is_none() && unlike.is_none() && pushed,
                    format!(
                        "a bone turned in first person: {turned:?}; the gun pushed unlike in third person: {unlike:?}; the gun pushed back: {pushed}"
                    ),
                )
            }),
            ("lean_needs_streak", {
                let leaned = [Run::streak([0.0]), Run::streak([0.0, 350.0])]
                    .iter()
                    .flat_map(|run| times(0.0, 3000.0).map(|now| run.lean(now)))
                    .fold(0.0f32, f32::max);
                check(
                    leaned == 0.0,
                    format!("one shot, then two 350 ms apart: the largest lean is {leaned}°"),
                )
            }),
            ("lean_sustained", {
                let run = sustained();
                let leans = |from, to| times(from, to).map(|now| run.lean(now));
                let before = leans(0.0, THIRD_MS - STEP_MS).fold(0.0f32, f32::max);
                let held =
                    leans(THIRD_MS + LEAN_IN_MS, LAST_MS + SUSTAIN_GAP_MS).fold(f32::MAX, f32::min);
                let easing: Vec<f32> = leans(
                    LAST_MS + SUSTAIN_GAP_MS,
                    LAST_MS + SUSTAIN_GAP_MS + LEAN_OUT_MS,
                )
                .collect();
                let falls = easing.windows(2).all(|pair| pair[1] <= pair[0]);
                let after = leans(LAST_MS + SUSTAIN_GAP_MS + LEAN_OUT_MS, LAST_MS + 3000.0)
                    .fold(0.0f32, f32::max);
                check(
                    before == 0.0 && (held - LEAN_DEG).abs() < 1e-5 && falls && after == 0.0,
                    format!(
                        "before the third shot {before}°; held at least {held}° from 300 ms after it until 1100 ms after the last; never rising while it eases out: {falls}; afterwards {after}°"
                    ),
                )
            }),
            ("lean_new_streak", {
                let start = LAST_MS + 1200.0;
                let run = Run::streak(sustained_times().chain([start, start + 350.0]));
                let streak = run.firing(start + 50.0).streak;
                let fresh =
                    streak.since_ms == start && streak.shots == 1 && streak.leaned_ms.is_none();
                let leaned = times(LAST_MS + SUSTAIN_GAP_MS + LEAN_OUT_MS, start + 2000.0)
                    .map(|now| run.lean(now))
                    .fold(0.0f32, f32::max);
                check(
                    fresh && leaned == 0.0,
                    format!(
                        "a new streak from the shot 1200 ms after the last: {fresh} ({streak:?}); its lean once the old one has eased out {leaned}°"
                    ),
                )
            }),
            ("lean_no_jolt", {
                let runs = [
                    sustained(),
                    Run::streak(sustained_times().chain([LAST_MS + 1200.0, LAST_MS + 1550.0])),
                ];
                let step = runs
                    .iter()
                    .flat_map(|run| {
                        let leans: Vec<f32> = times(0.0, 5000.0).map(|now| run.lean(now)).collect();
                        leans
                            .windows(2)
                            .map(|pair| (pair[1] - pair[0]).abs())
                            .collect::<Vec<_>>()
                    })
                    .fold(0.0f32, f32::max);
                check(step <= 0.2, format!("the largest step is {step:.3}°"))
            }),
            ("first_person_no_lean", {
                let run = Run {
                    looked_through: true,
                    ..sustained()
                };
                let chest = run
                    .whole()
                    .into_iter()
                    .find(|(_, turns)| angle(turns, CHEST) != (0.0, 0.0));
                let leans = angle(&sustained().at(LAST_MS), CHEST).0;
                check(
                    chest.is_none() && leans < -LEAN_DEG / 2.0,
                    format!(
                        "first person turns the chest: {chest:?}; third person leans it to {leans:.3}° at the last shot"
                    ),
                )
            }),
        ]
    }

    /// The third shot of `sustained`, and its last (ms).
    const THIRD_MS: f64 = 700.0;
    const LAST_MS: f64 = 1750.0;

    /// A rifle's shots every 350 ms for 2 s.
    fn sustained_times() -> impl Iterator<Item = f64> {
        (0..)
            .map(|n| n as f64 * 350.0)
            .take_while(|at| *at < 2000.0)
    }

    fn sustained() -> Run {
        Run::streak(sustained_times())
    }

    /// Every `STEP_MS` from `from_ms` to `to_ms`.
    fn times(from_ms: f64, to_ms: f64) -> impl Iterator<Item = f64> {
        (0..)
            .map(move |n| from_ms + n as f64 * STEP_MS)
            .take_while(move |now| *now <= to_ms)
    }
}
