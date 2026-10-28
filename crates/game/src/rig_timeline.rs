//! Timelines for the skeleton probes: JA animations cued on a Ghoul2 player
//! and baked (`asset_ja::g2`), with the ground travel that goes with them,
//! and the measures that say whether the result is clean (foot slide,
//! jolts).

use asset_odyssey::anim::{AnimationIr, NodeTrack};
use asset_odyssey::model::ModelIr;
use bevy::math::Vec3;
use fmt_ja::animation_cfg::{AnimationConfig, Sequence};
use fmt_ja::ghoul2::Gla;

use asset_ja::angles::{Angles, Steer};
use asset_ja::g2::{Cue, Part};

use sim::motion::{Anim, AnimTable};

use crate::rig_ground::{Ground, Hop, Leap, Plan, Wish};
use crate::saber_flight::Throw;

pub fn find<'c>(cfg: &'c AnimationConfig, name: &str) -> Result<&'c Sequence, String> {
    cfg.sequences
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or(format!("{name}: not in animation.cfg"))
}

/// The view at `ms` from its keys: held before the first and after the
/// last, eased in and out between two (smoothstep).
fn view_at(views: &[(f32, Angles)], ms: f32) -> Angles {
    let Some(after) = views.iter().position(|v| v.0 > ms) else {
        return views.last().map_or([0.0; 3], |v| v.1);
    };
    if after == 0 {
        return views[0].1;
    }
    let ((t0, a), (t1, b)) = (views[after - 1], views[after]);
    let f = ((ms - t0) / (t1 - t0).max(f32::EPSILON)).clamp(0.0, 1.0);
    let f = f * f * (3.0 - 2.0 * f);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f)
}

/// The movement direction from the movement keys: 0 ahead,
/// counting 45° steps to the left; with no key held a straight sideways
/// direction turns into its forward diagonal, so the legs do not stop
/// crooked. (ms, direction) from each key change.
fn movement_dirs(keys: &[(f32, i8, i8)]) -> Vec<(f32, u8)> {
    let mut dir = 0;
    keys.iter()
        .map(|&(ms, forward, right)| {
            dir = match (forward.signum(), right.signum()) {
                (1, 0) => 0,
                (1, -1) => 1,
                (0, -1) => 2,
                (-1, -1) => 3,
                (-1, 0) => 4,
                (-1, 1) => 5,
                (0, 1) => 6,
                (1, 1) => 7,
                _ => match dir {
                    2 => 1,
                    6 => 7,
                    other => other,
                },
            };
            (ms, dir)
        })
        .collect()
}

/// How long a sequence holds before the next move (`asset_ja::chain`).
fn hold_ms(gla: &Gla, sequence: &Sequence) -> Result<f32, String> {
    let (frames, ms) = asset_ja::playback(sequence, gla.frame_count)
        .ok_or(format!("{}: frames past the GLA", sequence.name))?;
    let held = if sequence.loop_frame != -1 {
        frames.len()
    } else {
        frames.len().saturating_sub(1).max(1)
    };
    Ok(held as f32 * ms)
}

/// How long a saber move holds before the next is cued, at `speed` (its
/// style's, `asset_ja::saber_anim_speed`).
fn move_hold(gla: &Gla, sequence: &Sequence, speed: f32) -> Result<f32, String> {
    let hold = hold_ms(gla, sequence)?;
    Ok(if speed == 1.0 {
        hold
    } else {
        asset_ja::move_hold_ms(hold, speed)
    })
}

/// How long the timelines hold the animation `name` as a move of the single
/// saber style `level` (1 fast, 2 medium, 3 strong; the dual and staff
/// styles play as medium): what `kjk combat saber` compares the sim's move
/// timers with.
#[cfg(feature = "probes")]
pub fn saber_move_hold(
    gla: &Gla,
    cfg: &AnimationConfig,
    level: u32,
    name: &str,
) -> Result<f32, String> {
    move_hold(
        gla,
        find(cfg, name)?,
        asset_ja::saber_anim_speed(level, name),
    )
}

/// Every 5 ms, as the probes bake.
const STEP_MS: f32 = 5.0;

/// The speed at which the ground must pass for a looping walk or run to
/// plant its feet on this body: how fast a foot that is down moves back
/// relative to the body, retargeted onto `body`, in metres per second.
pub fn natural_speed(gla: &Gla, sequence: &Sequence, body: &ModelIr) -> Result<f32, String> {
    let cycle = hold_ms(gla, sequence)?;
    let cues = [Cue {
        at_ms: 0.0,
        part: Part::Both,
        sequence,
        blend_ms: 0.0,
        speed_scale: 1.0,
    }];
    let baked = asset_ja::g2::bake(gla, &cues, cycle * 2.0, STEP_MS, &sequence.name);
    // Speed only: the feet's height does not matter here.
    let clip = asset_ja::rig::retarget(body, gla, &baked);
    let trace = asset_ja::rig::trace(body, &clip, &["lfoot_g", "rfoot_g"]);
    let mut speeds = Vec::new();
    for foot in 0..2 {
        let low = trace
            .iter()
            .map(|(_, p)| p[foot].z)
            .fold(f32::MAX, f32::min);
        for pair in trace.windows(2) {
            let ((t0, a), (t1, b)) = (&pair[0], &pair[1]);
            if a[foot].z < low + 0.02 && b[foot].z < low + 0.02 && t1 > t0 {
                speeds.push(-(b[foot].y - a[foot].y) / (t1 - t0));
            }
        }
    }
    if speeds.is_empty() {
        return Err(format!("{}: no foot stays down", sequence.name));
    }
    speeds.sort_by(f32::total_cmp);
    Ok(speeds[speeds.len() / 2])
}

/// Builds a timeline: cues placed one after another at the game's holds,
/// what the player asks of the ground, and leaps whose landings the sim's
/// movement times (`rig_ground`).
struct Builder<'c> {
    gla: &'c Gla,
    cfg: &'c AnimationConfig,
    cues: Vec<Cue<'c>>,
    plan: Plan,
    saber: Vec<(f32, bool)>,
    /// Where the next cue goes (ms).
    at: f32,
    /// What happened when, for the notes.
    log: Vec<String>,
    style: Style,
    throws: Vec<Throw>,
    /// When each throw is caught, as assumed (ms).
    catches: Vec<f32>,
    /// The single saber's style level (1 fast, 2 medium, 3 strong).
    level: u32,
    /// What the legs and the torso last played, for their blends.
    legs: Option<String>,
    torso: Option<String>,
    /// Where the player looks: (ms, angles) keys, eased between.
    views: Vec<(f32, Angles)>,
    /// The movement keys pressed from a moment on: (ms, forward, right).
    keys: Vec<(f32, i8, i8)>,
    /// Reproduce the look's jolting quirks (`asset_ja::angles`).
    quirks: bool,
    /// The force powers used, for their effects.
    powers: Vec<Power>,
    /// The animations' lengths, for the movement.
    anims: AnimTable,
}

impl<'c> Builder<'c> {
    fn new(gla: &'c Gla, cfg: &'c AnimationConfig, setup: &Setup) -> Builder<'c> {
        Builder {
            gla,
            cfg,
            cues: Vec::new(),
            plan: Plan::default(),
            saber: Vec::new(),
            at: 0.0,
            log: Vec::new(),
            style: Style::Single,
            throws: Vec::new(),
            catches: Vec::new(),
            level: 2,
            legs: None,
            torso: None,
            views: Vec::new(),
            keys: Vec::new(),
            quirks: setup.quirks,
            powers: Vec::new(),
            anims: crate::stage::table_of(gla, cfg),
        }
    }

    /// The ground the plan so far covers in `length_ms`, the view's yaw
    /// steering its keys.
    fn ground(&self, length_ms: f32) -> Ground {
        let views = &self.views;
        self.plan
            .run(length_ms, &self.anims, &|ms| view_at(views, ms)[1])
    }

    fn sequence(&self, name: &str) -> Result<&'c Sequence, String> {
        find(self.cfg, name)
    }

    /// Cues `name` on `part` now; returns its hold. A part already playing
    /// `name` carries on (JA sets the same animation again only when asked
    /// to restart). Each part blends as the cgame blends it
    /// from what it played last (`asset_ja::blend_ms`); transitions play at
    /// their style's speed, held as the move timer holds them
    /// (`asset_ja::saber_anim_speed`, `move_hold_ms`).
    fn cue(&mut self, part: Part, name: &str) -> Result<f32, String> {
        self.cue_with(part, name, false)
    }

    /// `cue`, starting `name` over on a part already playing it when
    /// `restart` (`SETANIM_FLAG_RESTART`).
    fn cue_with(&mut self, part: Part, name: &str, restart: bool) -> Result<f32, String> {
        let sequence = self.sequence(name)?;
        let speed = asset_ja::saber_anim_speed(self.level, name);
        let at = self.at;
        let mut push = |part: Part, blend_ms: f32| {
            self.cues.push(Cue {
                at_ms: at,
                part,
                sequence,
                blend_ms,
                speed_scale: speed,
            });
        };
        let set_legs = part != Part::Torso && (restart || self.legs.as_deref() != Some(name));
        let set_torso = part != Part::Legs && (restart || self.torso.as_deref() != Some(name));
        let legs = asset_ja::blend_ms(self.legs.as_deref(), name, false);
        let torso = asset_ja::blend_ms(self.torso.as_deref(), name, false);
        match (set_legs, set_torso) {
            (true, true) if legs == torso => push(Part::Both, legs),
            (true, true) => {
                push(Part::Legs, legs);
                push(Part::Torso, torso);
            }
            (true, false) => push(Part::Legs, legs),
            (false, true) => push(Part::Torso, torso),
            (false, false) => {}
        }
        if part != Part::Torso {
            self.legs = Some(name.to_string());
        }
        if part != Part::Legs {
            self.torso = Some(name.to_string());
        }
        move_hold(self.gla, sequence, speed)
    }

    /// Cues `name` and waits out its hold.
    fn play(&mut self, part: Part, name: &str) -> Result<(), String> {
        self.at += self.cue(part, name)?;
        Ok(())
    }

    /// Cues moves one after another (a combo); a move playing the last
    /// one's animation starts it over.
    fn chain(&mut self, part: Part, names: &[&str]) -> Result<(), String> {
        for name in names {
            self.at += self.cue_with(part, name, true)?;
        }
        Ok(())
    }

    /// Cues `name` and holds it `ms`.
    fn hold(&mut self, part: Part, name: &str, ms: f32) -> Result<(), String> {
        self.cue(part, name)?;
        self.at += ms;
        Ok(())
    }

    fn wish(&mut self, wish: Wish) {
        self.wish_at(self.at, wish);
    }

    fn wish_at(&mut self, ms: f32, wish: Wish) {
        self.plan.wishes.push((ms, wish));
    }

    /// When the origin first stands more than `units` above the ground
    /// after `from` (ms), rising.
    fn rises_past(&self, from: f32, units: f32) -> Option<f32> {
        let ground = self.ground(from + 10_000.0);
        let above = |t: f32| ground.offset(t / 1000.0).z / asset_ja::UNIT;
        (0..1250)
            .map(|i| from + i as f32 * 8.0)
            .find(|&t| above(t) > units && above(t + 8.0) > above(t))
    }

    /// Throws the right saber (the saber throw at `level`), the button held
    /// `hold_ms`: the torso guides it (`BOTH_SABERPULL`, held while it flies)
    /// and takes the ready stance again once it is
    /// caught. The catch is the flight's (`saber_flight::fly`, on the
    /// retargeted hand), given as `catch`; without it, a guess.
    fn throw(&mut self, hold_ms: f32, level: u32, catch: Option<f32>) -> Result<(), String> {
        let start = self.at;
        self.throws.push(Throw {
            start,
            release: start + hold_ms,
            level,
        });
        self.cue(Part::Torso, "BOTH_SABERPULL")?;
        self.at = catch
            .unwrap_or(start + hold_ms * 2.0 + 200.0)
            .max(start + hold_ms);
        self.catches.push(self.at);
        self.mark("caught");
        self.cue(Part::Torso, self.ready())?;
        Ok(())
    }

    /// An animation's whole length, all its frames, as a
    /// taunt or a kick is held.
    fn full(&self, name: &str) -> Result<f32, String> {
        let (frames, ms) = asset_ja::playback(self.sequence(name)?, self.gla.frame_count)
            .ok_or(format!("{name}: frames past the GLA"))?;
        Ok(frames.len() as f32 * ms)
    }

    /// A taunt, played for its whole length: on the
    /// whole body standing still, but `BOTH_ENGAGETAUNT` on the torso alone
    /// (JA's hand-extend taunt).
    fn taunt(&mut self, name: &str) -> Result<(), String> {
        self.mark(&name.to_ascii_lowercase());
        let part = if name == "BOTH_ENGAGETAUNT" {
            Part::Torso
        } else {
            Part::Both
        };
        let ms = self.full(name)?;
        self.hold(part, name, ms)
    }

    /// The melee stance: the legs stand (`BOTH_STAND1`) and JA
    /// remaps the torso to `BOTH_STAND6`.
    fn stance(&mut self, ms: f32) -> Result<(), String> {
        self.cue(Part::Legs, "BOTH_STAND1")?;
        self.hold(Part::Torso, "BOTH_STAND6", ms)
    }

    /// When the ground has settled after `from` (ms): the last landing, or
    /// `from` if nothing lands after it.
    fn settled(&self, from: f32) -> f32 {
        self.ground(from + 10_000.0)
            .landings
            .into_iter()
            .filter(|&t| t > from)
            .fold(from, f32::max)
    }

    /// Plays a saber move that carries the body, from now, and waits until
    /// it has played and the body has landed. While such a move plays, JA's
    /// game code clears the movement command and pushes the body itself:
    /// forward, sideways or up, within stretches read off the legs'
    /// animation timer. That timer starts a millisecond short of the move's
    /// hold and counts down; a stretch is named either by what the timer
    /// reads or by how far into the animation's full length it is. Logs the
    /// move, its span, the timer's start and the landings after it.
    fn driven(&mut self, name: &str) -> Result<(), String> {
        let start = self.at;
        let hold = self.cue(Part::Both, name)?;
        let sequence = self.sequence(name)?;
        let (frames, frame_ms) = asset_ja::playback(sequence, self.gla.frame_count)
            .ok_or(format!("{name}: frames past the GLA"))?;
        let length = frames.len() as f32 * frame_ms;
        let timer = hold - 1.0;
        // The moment the legs timer reads `r`.
        let when = |r: f32| start + timer - r;
        // `r` ms into the animation, counted from its full length.
        let after = |r: f32| start + r - (length - timer);
        // The run is 250 units/s; a medium attack moves at 0.85 of it, a
        // strong one at 0.55.
        let attack = 250.0 * 0.85;
        let strong = 250.0 * 0.55;
        let side = if name.ends_with("_LEFT") { -1.0 } else { 1.0 };
        let forward = |speed: f32| Wish::Velocity(Vec3::Y * speed);
        let stop = Wish::Velocity(Vec3::ZERO);
        self.wish(stop);

        let rise = |from: f32, to: f32, up: f32| Hop {
            from,
            to,
            leap: Leap::Rise(up),
        };
        match name {
            // The fast and the dual katas stand where they are.
            "BOTH_A1_SPECIAL" | "BOTH_A6_SABERPROTECT" => {}
            // The medium kata steps forward twice.
            "BOTH_A2_SPECIAL" => {
                self.wish_at(when(2700.0) + 1.0, forward(attack));
                self.wish_at(when(2300.0), stop);
                self.wish_at(when(900.0) + 1.0, forward(attack));
                self.wish_at(when(500.0), stop);
            }
            // The strong kata steps forward once.
            "BOTH_A3_SPECIAL" => {
                self.wish_at(when(1700.0) + 1.0, forward(strong));
                self.wish_at(when(1000.0), stop);
            }
            // The staff kata creeps forward at 64/127 of a full command and
            // hops once on the way.
            "BOTH_A7_SOULCAL" => {
                self.wish(Wish::Velocity(Vec3::Y * attack * 64.0 / 127.0));
                self.wish_at(when(2750.0) + 1.0, stop);
                self.plan
                    .hops
                    .push(rise(when(2850.0) + 1.0, when(2650.0) + 1.0, 250.0));
            }
            "BOTH_BUTTERFLY_FL1" | "BOTH_BUTTERFLY_FR1" => {
                self.wish_at(after(250.0), forward(attack));
                self.wish_at(when(100.0) + 1.0, stop);
                self.plan
                    .hops
                    .push(rise(when(1800.0) + 1.0, when(1700.0) + 1.0, 250.0));
            }
            // The side butterflies launch sideways at half the run; the left
            // and the right one are not mirror images (the game's own data).
            "BOTH_BUTTERFLY_LEFT" | "BOTH_BUTTERFLY_RIGHT" => {
                let (up, hop) = if side < 0.0 {
                    (250.0, rise(when(1400.0) + 1.0, when(1200.0) + 1.0, 350.0))
                } else {
                    (350.0, rise(when(1800.0) + 1.0, when(1700.0) + 1.0, 250.0))
                };
                self.plan
                    .leaps
                    .push((start, Leap::Launch(Vec3::new(side * 190.0, 0.0, up))));
                self.wish(Wish::Velocity(Vec3::X * side * 125.0));
                self.wish_at(when(450.0), stop);
                self.plan.hops.push(hop);
            }
            // The dual sabers' jump attack: a push forward and two hops.
            "BOTH_JUMPATTACK6" => {
                self.wish_at(after(250.0), forward(attack));
                self.wish_at(when(100.0) + 1.0, stop);
                self.plan
                    .hops
                    .push(rise(after(950.0), when(900.0) + 1.0, 250.0));
                self.plan
                    .hops
                    .push(rise(after(400.0), when(1600.0) + 1.0, 250.0));
            }
            // The staff's back flip: straight up, then thrown back.
            "BOTH_JUMPATTACK7" => {
                self.plan.leaps.push((start, Leap::Rise(500.0)));
                self.plan.hops.push(Hop {
                    from: after(400.0),
                    to: when(800.0),
                    leap: Leap::Launch(Vec3::new(0.0, -100.0, 300.0)),
                });
            }
            _ => return Err(format!("{name}: not a saber move that carries the body")),
        }

        self.at = self.settled(start).max(start + hold);
        let landings: Vec<String> = self
            .ground(self.at)
            .landings
            .iter()
            .filter(|&&t| t > start)
            .map(|t| format!("{:.0}", t - start))
            .collect();
        self.log.push(format!(
            "{} {start:.0}..{:.0} (legs timer {timer:.0}, lands {})",
            name.to_ascii_lowercase(),
            self.at,
            landings.join(" ")
        ));
        Ok(())
    }

    /// The fast style's lunge (then move_body's hold on the command): the body is set moving 150 units/s along the
    /// view after the move of the step that chose it; from the next step
    /// the forward key is held at full speed (250: the fast style keeps
    /// its speed attacking), strafe and jump cleared, while the legs timer
    /// reads over 500 ms; then nothing is pressed.
    fn lunge(&mut self) -> Result<(), String> {
        let start = self.at;
        self.plan.leaps.push((start, Leap::Launch(Vec3::Y * 150.0)));
        let hold = self.cue(Part::Both, "BOTH_LUNGE2_B__T_")?;
        // The legs timer starts one frame short, less 1 ms (`driven`).
        let timer = hold - 1.0;
        self.wish_at(
            start + crate::rig_ground::STEP,
            Wish::Velocity(Vec3::Y * 250.0),
        );
        self.wish_at(start + timer - 500.0, Wish::Velocity(Vec3::ZERO));
        self.at = self.settled(start).max(start + hold);
        self.log.push(format!(
            "lunge {start:.0}..{:.0} (pushed forward until {:.0})",
            self.at,
            start + timer - 500.0
        ));
        Ok(())
    }

    /// Leaps now; returns when move_body lands it (ms, logged), or now for a
    /// push along the ground.
    fn leap(&mut self, leap: Leap) -> Result<f32, String> {
        self.plan.leaps.push((self.at, leap));
        if matches!(leap, Leap::Launch(v) if v.z <= 0.0) {
            return Ok(self.at);
        }
        let from = self.at;
        let land = self
            .ground(self.at + 10_000.0)
            .landings
            .into_iter()
            .find(|&t| t > from)
            .ok_or("a leap that never lands".to_string())?;
        self.log.push(format!("lands {land:.0}"));
        Ok(land)
    }

    /// Rolls now (the whole body, held the animation's
    /// length); move_body drives the roll from there.
    fn roll(&mut self, name: &str) -> Result<(), String> {
        let anim = Anim::from_name(name).ok_or(format!("{name}: not a sim animation"))?;
        self.plan.rolls.push((self.at, anim));
        Ok(())
    }

    fn mark(&mut self, what: &str) {
        self.log.push(format!("{what} {:.0}", self.at));
    }

    /// The style's ready stance, draw and put-away: JA
    /// swaps the single saber's for the staff's and the dual sabers'.
    fn ready(&self) -> &'static str {
        match self.style {
            Style::Single => "BOTH_STAND2",
            Style::Dual => "BOTH_SABERDUAL_STANCE",
            Style::Staff => "BOTH_SABERSTAFF_STANCE",
            Style::Melee => "BOTH_STAND6",
        }
    }

    /// Saber in hand and off, then drawn: the blade lights as the draw
    /// starts (the saber turns on with the command).
    fn draw(&mut self) -> Result<(), String> {
        self.hold(Part::Both, "BOTH_STAND1", 300.0)?;
        self.mark("draw");
        self.saber.push((self.at, true));
        let draw = match self.style {
            Style::Single | Style::Melee => "BOTH_STAND1TO2",
            Style::Dual => "BOTH_S1_S6",
            Style::Staff => "BOTH_S1_S7",
        };
        self.play(Part::Both, draw)?;
        self.hold(Part::Both, self.ready(), 400.0)
    }

    fn put_away(&mut self) -> Result<(), String> {
        self.mark("put away");
        self.saber.push((self.at, false));
        let away = match self.style {
            Style::Single | Style::Melee => "BOTH_STAND2TO1",
            Style::Dual => "BOTH_S6_S1",
            Style::Staff => "BOTH_S7_S1",
        };
        self.play(Part::Both, away)?;
        self.hold(Part::Both, "BOTH_STAND1", 400.0)
    }

    /// Uses a force power from now for `ms`.
    fn power(&mut self, kind: PowerKind, ms: f32, level: u32) {
        self.powers.push(Power {
            kind,
            start: self.at,
            end: self.at + ms,
            level,
        });
    }

    /// Turns the view to `pitch` and `yaw` (degrees, Quake's: pitch down,
    /// yaw left of the start) over `ms` from now, easing in and out as a
    /// hand on a mouse would; does not wait.
    fn view(&mut self, pitch: f32, yaw: f32, ms: f32) {
        let from = view_at(&self.views, self.at);
        self.views.push((self.at, from));
        self.views.push((self.at + ms, [pitch, yaw, 0.0]));
    }

    /// The legs' animation for the movement keys (as JA picks them with the
    /// saber lit): backward when `forward` is held back,
    /// walking with the walk key. The staff's and
    /// the dual sabers' run backward as the single saber does, and walk
    /// backward their own way; unarmed runs and walks the plain way.
    fn footsteps(&self, forward: i8, walking: bool) -> &'static str {
        let back = forward < 0;
        match (self.style, walking, back) {
            (Style::Melee, false, false) => "BOTH_RUN1",
            (Style::Melee, false, true) => "BOTH_RUNBACK1",
            (Style::Melee, true, false) => "BOTH_WALK1",
            (Style::Melee, true, true) => "BOTH_WALKBACK1",
            (_, false, true) => "BOTH_RUNBACK2",
            (Style::Single, false, false) => "BOTH_RUN2",
            (Style::Staff, false, false) => "BOTH_RUN_STAFF",
            (Style::Dual, false, false) => "BOTH_RUN_DUAL",
            (Style::Single, true, false) => "BOTH_WALK2",
            (Style::Single, true, true) => "BOTH_WALKBACK2",
            (Style::Staff, true, false) => "BOTH_WALK_STAFF",
            (Style::Staff, true, true) => "BOTH_WALKBACK_STAFF",
            (Style::Dual, true, false) => "BOTH_WALK_DUAL",
            (Style::Dual, true, true) => "BOTH_WALKBACK_DUAL",
        }
    }

    /// Holds the movement keys `forward` and `right` (−1, 0 or 1) for `ms`
    /// at `speed` (m/s): move_body moves along them as the view points them,
    /// at 0.75 of the speed running backward, the
    /// same speed on a diagonal; the legs play the run or
    /// walk (`footsteps`) and the torso follows them (`LS_READY` with a
    /// moving legs animation).
    fn press(
        &mut self,
        forward: i8,
        right: i8,
        walking: bool,
        speed: f32,
        ms: f32,
    ) -> Result<(), String> {
        let start = self.at;
        self.keys.push((start, forward, right));
        let name = self.footsteps(forward, walking);
        self.cue(Part::Both, name)?;
        self.wish(Wish::Keys {
            forward,
            right,
            walking,
            speed: speed / asset_ja::UNIT,
        });
        self.at = start + ms;
        Ok(())
    }

    /// Lets go of the movement keys and holds `ms`: the legs keep running
    /// until the ground speed falls under 5 units/s, then stand in the
    /// style's ready stance.
    fn release(&mut self, ms: f32) -> Result<(), String> {
        let start = self.at;
        self.keys.push((start, 0, 0));
        self.wish(Wish::Velocity(Vec3::ZERO));
        let ground = self.ground(start + ms + 2000.0);
        let stopped = (0..)
            .map(|i| start + i as f32 * 8.0)
            .find(|&t| {
                let [x, y, _] = ground.velocity(t / 1000.0);
                (x * x + y * y).sqrt() < 5.0 || t > start + 2000.0
            })
            .unwrap_or(start);
        self.at = stopped;
        self.hold(Part::Both, self.ready(), (start + ms - stopped).max(0.0))
    }

    fn finish(self, name: &str, notes: &mut Vec<String>) -> Timeline {
        let length = self.at;
        let ground = self.ground(length);
        notes.push(format!(
            "{name}: {}; {length:.0} ms, ends {:.2} m from the start",
            self.log.join(", "),
            ground.offset(length / 1000.0).truncate().length()
        ));
        let moves = self
            .cues
            .iter()
            .filter(|cue| cue.part != Part::Legs)
            .map(|cue| (cue.at_ms, cue.sequence.name.clone()))
            .collect();
        let dirs = movement_dirs(&self.keys);
        let views = &self.views;
        let steer = |ms: f32| {
            let t = ms / 1000.0;
            Steer {
                view: view_at(views, ms),
                velocity: ground.velocity(t),
                move_dir: dirs.iter().rfind(|d| d.0 <= ms).map_or(0, |d| d.1),
                on_ground: ground.on_ground(t),
                look: None,
            }
        };
        let ready = if self.style == Style::Melee {
            "TORSO_WEAPONREADY3"
        } else {
            "BOTH_STAND2"
        };
        let look = asset_ja::g2::Look {
            steer: &steer,
            ready,
            quirks: self.quirks,
        };
        let steered =
            asset_ja::g2::bake_steered(self.gla, &self.cues, length, STEP_MS, name, &look);
        notes.push(angle_steps(&steered.angles));
        Timeline {
            clip: steered.clip,
            angles: steered.angles,
            powers: self.powers,
            ground,
            saber: self.saber,
            moves,
            style: self.style,
            throws: self.throws,
            catches: self.catches,
        }
    }
}

/// The built-in timelines `kjk rig --moves <name>` plays.
pub const TIMELINES: &[&str] = &[
    "showcase",
    "acrobatics",
    "specials",
    "defense",
    "force",
    "staff",
    "dual",
    "reactions",
    "melee",
    "styles",
    "gestures",
    "look",
];

/// How a timeline is built besides its moves.
pub struct Setup<'a> {
    /// When its thrown sabers come back (ms), once flown.
    pub catches: &'a [f32],
    /// Reproduce the look's jolting quirks (`asset_ja::angles`).
    pub quirks: bool,
}

/// The timeline `name`.
pub fn timeline(
    name: &str,
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    match name {
        "showcase" => showcase(gla, cfg, body, setup, notes),
        "acrobatics" => acrobatics(gla, cfg, body, setup, notes),
        "specials" => specials(gla, cfg, setup, notes),
        "defense" => defense(gla, cfg, setup, notes),
        "force" => force(gla, cfg, setup, notes),
        "staff" => two_bladed(Style::Staff, gla, cfg, body, setup, notes),
        "dual" => two_bladed(Style::Dual, gla, cfg, body, setup, notes),
        "reactions" => reactions(gla, cfg, setup, notes),
        "melee" => melee(gla, cfg, body, setup, notes),
        "styles" => styles(gla, cfg, setup, notes),
        "gestures" => gestures(gla, cfg, setup, notes),
        "look" => look(gla, cfg, body, setup, notes),
        other => Err(format!(
            "{other}: no such timeline ({})",
            TIMELINES.join(", ")
        )),
    }
}

/// The staff's (style 7) or the dual sabers' (6) combo: their own start,
/// attack, transition, attack and return.
pub fn two_bladed_combo(style: u32) -> [String; 5] {
    let n = style;
    [
        format!("BOTH_S{n}_S{n}_TL"),
        format!("BOTH_A{n}_TL_BR"),
        format!("BOTH_T{n}_BR_TR"),
        format!("BOTH_A{n}_TR_BL"),
        format!("BOTH_R{n}_BL_S{n}"),
    ]
}

/// The staff or the dual sabers: draw, walk and run with them, their combo
/// (their own start, attack, transition, attack and return: style 7 and 6),
/// their spin attack, the dual forward jump attack or
/// the staff's back-flip attack, the staff's kata, put away.
pub fn two_bladed(
    style: Style,
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let n = if style == Style::Staff { 7 } else { 6 };
    let (walk, run) = if style == Style::Staff {
        ("BOTH_WALK_STAFF", "BOTH_RUN_STAFF")
    } else {
        ("BOTH_WALK_DUAL", "BOTH_RUN_DUAL")
    };
    let walk_speed = natural_speed(gla, find(cfg, walk)?, body)?;
    let run_speed = natural_speed(gla, find(cfg, run)?, body)?;
    notes.push(format!(
        "natural speed on {}: {} {walk_speed:.2} m/s, {} {run_speed:.2} m/s",
        body.name,
        walk.to_ascii_lowercase(),
        run.to_ascii_lowercase()
    ));
    let mut b = Builder::new(gla, cfg, setup);
    b.style = style;
    b.draw()?;
    b.mark("walk");
    b.wish(Wish::Velocity(Vec3::Y * walk_speed / asset_ja::UNIT));
    b.hold(Part::Both, walk, 1000.0)?;
    b.mark("run");
    b.wish(Wish::Velocity(Vec3::Y * run_speed / asset_ja::UNIT));
    b.hold(Part::Both, run, 1000.0)?;
    b.wish(Wish::Velocity(Vec3::ZERO));
    b.hold(Part::Both, b.ready(), 400.0)?;
    b.mark("combo");
    for name in &two_bladed_combo(n) {
        b.play(Part::Both, name)?;
    }
    b.hold(Part::Both, b.ready(), 400.0)?;
    b.mark("spin attack");
    b.play(Part::Both, &format!("BOTH_SPINATTACK{n}"))?;
    b.hold(Part::Both, b.ready(), 400.0)?;
    b.mark("kata");
    b.driven(if style == Style::Staff {
        "BOTH_A7_SOULCAL"
    } else {
        "BOTH_A6_SABERPROTECT"
    })?;
    b.hold(Part::Both, b.ready(), 400.0)?;
    // The jump attacks (forward: the dual sabers' and the staff's; back:
    // the staff's back flip) and the butterflies, as JA's saber moves
    // drive them. Without saber overrides forward is always the staff's
    // right one.
    b.mark("jump attack");
    if style == Style::Dual {
        b.driven("BOTH_JUMPATTACK6")?;
    } else {
        b.driven("BOTH_BUTTERFLY_FR1")?;
        b.hold(Part::Both, b.ready(), 400.0)?;
        b.mark("back flip attack");
        b.driven("BOTH_JUMPATTACK7")?;
    }
    b.hold(Part::Both, b.ready(), 400.0)?;
    if style == Style::Staff {
        b.mark("butterflies");
        b.driven("BOTH_BUTTERFLY_LEFT")?;
        b.hold(Part::Both, b.ready(), 400.0)?;
        b.driven("BOTH_BUTTERFLY_RIGHT")?;
        b.hold(Part::Both, b.ready(), 400.0)?;
        // The staff's kicks.
        for kick in ["BOTH_A7_KICK_F", "BOTH_A7_KICK_R", "BOTH_A7_KICK_B"] {
            b.mark(&kick.to_ascii_lowercase());
            b.play(Part::Both, kick)?;
            b.hold(Part::Both, b.ready(), 300.0)?;
        }
    }
    // Their taunts: taunt, flourish, gloat (`G_SetTauntAnim`).
    let (taunt, flourish, gloat) = if style == Style::Staff {
        (
            "BOTH_STAFF_TAUNT",
            "BOTH_SHOWOFF_STAFF",
            "BOTH_VICTORY_STAFF",
        )
    } else {
        ("BOTH_DUAL_TAUNT", "BOTH_SHOWOFF_DUAL", "BOTH_VICTORY_DUAL")
    };
    for name in [taunt, flourish, gloat] {
        b.taunt(name)?;
        b.hold(Part::Both, b.ready(), 400.0)?;
    }
    b.put_away()?;
    let name = if style == Style::Staff {
        "staff"
    } else {
        "dual"
    };
    Ok(b.finish(name, notes))
}

/// Being hit and what follows: flinches (torso, then whole body), a saber
/// lock held and broken, a knockdown and getting up, a knockdown and a
/// force flip up, and a death held on its last frame. In place: knockback
/// is the sim's (I13).
pub fn reactions(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("pain");
    b.play(Part::Torso, "BOTH_PAIN1")?;
    b.hold(Part::Both, "BOTH_STAND2", 300.0)?;
    b.play(Part::Both, "BOTH_PAIN2")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("saber lock");
    b.hold(Part::Both, "BOTH_BF2LOCK", 1200.0)?;
    b.play(Part::Both, "BOTH_BF2BREAK")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("knockdown");
    b.play(Part::Both, "BOTH_KNOCKDOWN1")?;
    b.play(Part::Both, "BOTH_GETUP1")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("force getup");
    b.play(Part::Both, "BOTH_KNOCKDOWN2")?;
    b.play(Part::Both, "BOTH_FORCE_GETUP_B1")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("death");
    b.play(Part::Both, "BOTH_DEATH1")?;
    b.hold(Part::Both, "BOTH_DEAD1", 1000.0)?;
    b.saber.push((b.at, false));
    b.hold(Part::Both, "BOTH_DEAD1", 600.0)?;
    Ok(b.finish("reactions", notes))
}

/// Unarmed, as JA multiplayer fights with bare hands:
/// - the melee stance, a walk and a run (`BOTH_WALK1`, `BOTH_RUN1`, the
///   torso following the legs);
/// - punches while attack is held: `BOTH_MELEE1` and `BOTH_MELEE2` in turn
///   on the torso, one every 400 ms (the weapon's fire time), each cutting
///   the last short;
/// - the kicks `g_debugMelee` allows (the staff's kick animations, held
///   their full length, movement frozen);
/// - an air kick, which needs more than 64 units under the origin, so from
///   a force jump; landing in it plays its force landing.
pub fn melee(
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let walk_speed = natural_speed(gla, find(cfg, "BOTH_WALK1")?, body)?;
    let run_speed = natural_speed(gla, find(cfg, "BOTH_RUN1")?, body)?;
    notes.push(format!(
        "natural speed on {}: walk1 {walk_speed:.2} m/s, run1 {run_speed:.2} m/s",
        body.name
    ));
    let full = |name: &str| -> Result<f32, String> {
        let (frames, ms) = asset_ja::playback(find(cfg, name)?, gla.frame_count)
            .ok_or(format!("{name}: frames past the GLA"))?;
        Ok(frames.len() as f32 * ms)
    };
    let mut b = Builder::new(gla, cfg, setup);
    b.style = Style::Melee;
    b.stance(600.0)?;
    b.mark("walk");
    b.wish(Wish::Velocity(Vec3::Y * walk_speed / asset_ja::UNIT));
    b.hold(Part::Both, "BOTH_WALK1", 1000.0)?;
    b.mark("run");
    b.wish(Wish::Velocity(Vec3::Y * run_speed / asset_ja::UNIT));
    b.hold(Part::Both, "BOTH_RUN1", 1000.0)?;
    b.wish(Wish::Velocity(Vec3::ZERO));
    b.stance(500.0)?;
    b.mark("punches");
    b.cue(Part::Legs, "BOTH_STAND1")?;
    for punch in ["BOTH_MELEE1", "BOTH_MELEE2", "BOTH_MELEE1", "BOTH_MELEE2"] {
        b.hold(Part::Torso, punch, 400.0)?;
    }
    b.stance(500.0)?;
    for kick in [
        "BOTH_A7_KICK_F",
        "BOTH_A7_KICK_B",
        "BOTH_A7_KICK_R",
        "BOTH_A7_KICK_L",
    ] {
        b.mark(&kick.to_ascii_lowercase());
        b.hold(Part::Both, kick, full(kick)?)?;
        b.stance(300.0)?;
    }
    b.mark("air kick");
    let land = b.leap(Leap::Force {
        height: 96.0,
        strength: 420.0,
    })?;
    let kick_at = b
        .rises_past(b.at, 64.0)
        .ok_or("a force jump that never rises 64 units")?;
    // The torso follows the legs while not attacking.
    b.cue(Part::Both, "BOTH_FORCEJUMP1")?;
    b.at = kick_at;
    let kick = "BOTH_A7_KICK_F_AIR";
    let kick_end = b.at + full(kick)?;
    b.mark("kick");
    b.cue(Part::Both, kick)?;
    if land <= kick_end {
        b.at = land;
        b.mark("force land");
        b.play(Part::Both, "BOTH_FORCELAND1")?;
    } else {
        b.at = kick_end;
        b.cue(Part::Both, "BOTH_FORCEINAIR1")?;
        b.at = land;
        b.mark("land");
        b.play(Part::Both, "BOTH_LAND1")?;
    }
    b.stance(600.0)?;
    Ok(b.finish("melee", notes))
}

/// The fast and the strong styles' combos, each from its stance,
/// chained as JA lets them: the fast
/// style three attacks (up to five spinning ones chain), the strong style
/// two, the second carrying the first's momentum on (a 180° chain angle,
/// `saberMoveTransitionAngle`).
pub fn styles(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("fast");
    b.level = 1;
    b.hold(Part::Both, "BOTH_SABERFAST_STANCE", 500.0)?;
    b.chain(Part::Both, FAST_COMBO)?;
    b.hold(Part::Both, "BOTH_SABERFAST_STANCE", 500.0)?;
    b.mark("fast kata");
    b.driven("BOTH_A1_SPECIAL")?;
    b.hold(Part::Both, "BOTH_SABERFAST_STANCE", 500.0)?;
    b.mark("medium kata");
    b.level = 2;
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.driven("BOTH_A2_SPECIAL")?;
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.mark("strong");
    b.level = 3;
    b.hold(Part::Both, "BOTH_SABERSLOW_STANCE", 600.0)?;
    b.chain(Part::Both, STRONG_COMBO)?;
    b.hold(Part::Both, "BOTH_SABERSLOW_STANCE", 500.0)?;
    b.mark("strong kata");
    b.driven("BOTH_A3_SPECIAL")?;
    b.hold(Part::Both, "BOTH_SABERSLOW_STANCE", 500.0)?;
    b.level = 2;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.put_away()?;
    Ok(b.finish("styles", notes))
}

/// The taunts multiplayer gives the medium style (`G_SetTauntAnim`): the
/// taunt (torso only), the flourish and the gloat; the bow and meditation
/// put the saber away (`saberHolstered` 2); meditation holds until a button
/// ends it (`BOTH_MEDITATE_END`); a flourish lights the saber again.
pub fn gestures(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    for taunt in [
        "BOTH_ENGAGETAUNT",
        "BOTH_SHOWOFF_MEDIUM",
        "BOTH_VICTORY_MEDIUM",
    ] {
        b.taunt(taunt)?;
        b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    }
    b.saber.push((b.at, false));
    b.taunt("BOTH_BOW")?;
    b.hold(Part::Both, "BOTH_STAND1", 400.0)?;
    b.mark("meditate");
    b.hold(Part::Both, "BOTH_MEDITATE", 2000.0)?;
    b.play(Part::Both, "BOTH_MEDITATE_END")?;
    b.hold(Part::Both, "BOTH_STAND1", 400.0)?;
    b.saber.push((b.at, true));
    b.taunt("BOTH_SHOWOFF_MEDIUM")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.put_away()?;
    Ok(b.finish("gestures", notes))
}

/// Looking and moving about with the saber ready, the view scripted as a
/// player's mouse and keys would move it (`asset_ja::angles`):
/// - standing, looking left and right (the legs turn with the view: JA's
///   multiplayer has no turning animations) and up and down (the spine
///   bends, the head stays nearly level);
/// - turning round in place and back;
/// - running ahead, strafing right and left, diagonally ahead and back,
///   and backward, the view held: the legs turn toward the line of travel
///   (at most 60°) and the spine turns the rest back to the view;
/// - running while the view sweeps round, and stopping.
pub fn look(
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let run = natural_speed(gla, find(cfg, "BOTH_RUN2")?, body)?;
    notes.push(format!(
        "natural speed on {}: run {run:.2} m/s (backward 0.75 of it)",
        body.name
    ));
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("look around");
    b.view(0.0, 70.0, 700.0);
    b.hold(Part::Both, b.ready(), 1000.0)?;
    b.view(0.0, -70.0, 1200.0);
    b.at += 1500.0;
    b.view(0.0, 0.0, 700.0);
    b.at += 1000.0;
    b.mark("look up and down");
    b.view(-60.0, 0.0, 600.0);
    b.at += 1000.0;
    b.view(60.0, 0.0, 1000.0);
    b.at += 1400.0;
    b.view(0.0, 0.0, 600.0);
    b.at += 900.0;
    b.mark("turn round");
    b.view(0.0, 180.0, 1000.0);
    b.at += 1400.0;
    b.view(0.0, 0.0, 1000.0);
    b.at += 1400.0;
    for (what, forward, right, ms) in [
        ("run", 1, 0, 800.0),
        ("strafe right", 0, 1, 1000.0),
        ("strafe left", 0, -1, 1000.0),
        ("ahead left", 1, -1, 800.0),
        ("back right", -1, 1, 1000.0),
        ("back", -1, 0, 800.0),
        ("back left", -1, -1, 800.0),
    ] {
        b.mark(what);
        b.press(forward, right, false, run, ms)?;
    }
    b.mark("stop");
    b.release(800.0)?;
    b.mark("run turning");
    b.view(0.0, 120.0, 1600.0);
    b.press(1, 0, false, run, 2000.0)?;
    b.release(600.0)?;
    b.view(0.0, 0.0, 1000.0);
    b.at += 1200.0;
    b.put_away()?;
    Ok(b.finish("look", notes))
}

/// The fast style's combo: three attacks, spinning ones chained.
pub const FAST_COMBO: &[&str] = &[
    "BOTH_S1_S1_TL",
    "BOTH_A1_TL_BR",
    "BOTH_T1_BR_TR",
    "BOTH_A1_TR_BL",
    "BOTH_T1_BL_T_",
    "BOTH_A1_T__B_",
    "BOTH_R1_B__S1",
];

/// The strong style's combo: two attacks, the second carrying the first's
/// momentum on.
pub const STRONG_COMBO: &[&str] = &[
    "BOTH_S3_S1_TL",
    "BOTH_A3_TL_BR",
    "BOTH_T3_BR_TL",
    "BOTH_A3_TL_BR",
    "BOTH_R3_BR_S1",
];

/// The medium style's combo from the ready stance.
pub const MEDIUM_COMBO: &[&str] = &[
    "BOTH_S2_S1_TL",
    "BOTH_A2_TL_BR",
    "BOTH_T2_BR_TR",
    "BOTH_A2_TR_BL",
    "BOTH_R2_BL_S1",
];

/// The showcase: draw, walk, run, a swing while running (torso only, the
/// legs keep running), stop, the medium-style combo standing, a run
/// backward, a jump, and the saber put away. Walk and run wish for their
/// natural speed on `body`, so the feet stay planted.
pub fn showcase(
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let walk_speed = natural_speed(gla, find(cfg, "BOTH_WALK2")?, body)?;
    let run_speed = natural_speed(gla, find(cfg, "BOTH_RUN2")?, body)?;
    let back_speed = natural_speed(gla, find(cfg, "BOTH_RUNBACK2")?, body)?;
    notes.push(format!(
        "natural speed on {}: walk {walk_speed:.2} m/s, run {run_speed:.2} m/s, run back {back_speed:.2} m/s",
        body.name
    ));
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("walk");
    b.wish(Wish::Velocity(Vec3::Y * walk_speed / asset_ja::UNIT));
    b.hold(Part::Both, "BOTH_WALK2", 1000.0)?;
    b.mark("run");
    b.wish(Wish::Velocity(Vec3::Y * run_speed / asset_ja::UNIT));
    b.hold(Part::Both, "BOTH_RUN2", 300.0)?;
    // A swing while running: start, attack, return on the torso.
    b.mark("swing on the run");
    b.chain(
        Part::Torso,
        &["BOTH_S2_S1_TL", "BOTH_A2_TL_BR", "BOTH_R2_BR_S1"],
    )?;
    b.mark("stop");
    b.wish(Wish::Velocity(Vec3::ZERO));
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("combo");
    b.chain(Part::Both, MEDIUM_COMBO)?;
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.mark("run back");
    b.wish(Wish::Velocity(Vec3::Y * back_speed / asset_ja::UNIT));
    b.hold(Part::Both, "BOTH_RUNBACK2", 800.0)?;
    b.wish(Wish::Velocity(Vec3::ZERO));
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    // A jump: the legs take off (`BOTH_JUMP1`) and hang (`BOTH_INAIR1`),
    // and land (`BOTH_LAND1`, legs only); the torso
    // stays ready.
    b.mark("jump");
    let land = b.leap(Leap::Jump)?;
    b.play(Part::Legs, "BOTH_JUMP1")?;
    b.cue(Part::Legs, "BOTH_INAIR1")?;
    b.at = land;
    b.play(Part::Legs, "BOTH_LAND1")?;
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.put_away()?;
    Ok(b.finish("showcase", notes))
}

/// Crouch, crouch-walk, rolls four ways, a force jump and flips forward and
/// back. Crouching is on the legs (JA sets `BOTH_CROUCH1*` on
/// the legs), rolls and flips on the whole body; a force jump rises
/// on `BOTH_FORCEJUMP1` (legs), hangs on `BOTH_FORCEINAIR1` and lands on
/// `BOTH_LAND1` (whole body, as JA lands force jumps).
pub fn acrobatics(
    gla: &Gla,
    cfg: &AnimationConfig,
    body: &ModelIr,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let crawl = natural_speed(gla, find(cfg, "BOTH_CROUCH1WALK")?, body)?;
    let run_speed = natural_speed(gla, find(cfg, "BOTH_RUN2")?, body)?;
    notes.push(format!(
        "natural speed on {}: crouch walk {crawl:.2} m/s, run {run_speed:.2} m/s",
        body.name
    ));
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("crouch");
    b.play(Part::Legs, "BOTH_CROUCH1")?;
    b.hold(Part::Legs, "BOTH_CROUCH1IDLE", 300.0)?;
    b.wish(Wish::Velocity(Vec3::Y * crawl / asset_ja::UNIT));
    b.hold(Part::Legs, "BOTH_CROUCH1WALK", 1000.0)?;
    b.wish(Wish::Velocity(Vec3::ZERO));
    b.hold(Part::Legs, "BOTH_CROUCH1IDLE", 300.0)?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    for name in ["BOTH_ROLL_F", "BOTH_ROLL_R", "BOTH_ROLL_L", "BOTH_ROLL_B"] {
        b.mark(&name.to_ascii_lowercase());
        let hold_ms = b.cue(Part::Both, name)?;
        b.roll(name)?;
        b.at += hold_ms;
        b.wish(Wish::Velocity(Vec3::ZERO));
        b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    }
    b.mark("force jump");
    let land = b.leap(Leap::Force {
        height: 96.0,
        strength: 420.0,
    })?;
    b.play(Part::Legs, "BOTH_FORCEJUMP1")?;
    b.cue(Part::Legs, "BOTH_FORCEINAIR1")?;
    b.at = land;
    b.play(Part::Both, "BOTH_LAND1")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    for (name, direction) in [("BOTH_FLIP_F", Vec3::Y), ("BOTH_FLIP_B", -Vec3::Y)] {
        b.mark(&name.to_ascii_lowercase());
        b.wish(Wish::Velocity(direction * run_speed / asset_ja::UNIT));
        let land = b.leap(Leap::Force {
            height: 96.0,
            strength: 420.0,
        })?;
        b.play(Part::Both, name)?;
        if b.at < land {
            b.cue(Part::Both, "BOTH_FORCEINAIR1")?;
        }
        b.at = b.at.max(land);
        b.wish(Wish::Velocity(Vec3::ZERO));
        b.play(Part::Both, "BOTH_LAND1")?;
        b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    }
    // Cartwheels (as JA's movement attacks): jump, strafe and a tap of
    // attack together. The jump (225 up) is what lets the attack through,
    // in the same frame, so the cgame never sees the jump's animation: the
    // legs go straight into cartwheeling (`BOTH_ARIAL_*`, the legs only:
    // the torso keeps its stance, and the saber move is not set) and the
    // body flies 190 units/s sideways and 300 up (right) or 350 (left). A
    // special jump: landing does not cut it short; if it ends in the air
    // the legs hang in `BOTH_INAIR1` and land in `BOTH_LAND1`.
    for (name, side, up) in [
        ("BOTH_ARIAL_RIGHT", 1.0, 300.0),
        ("BOTH_ARIAL_LEFT", -1.0, 350.0),
    ] {
        b.mark(&name.to_ascii_lowercase());
        b.wish(Wish::Velocity(Vec3::X * side * 250.0));
        let land = b.leap(Leap::Launch(Vec3::new(side * 190.0, 0.0, up)))?;
        let end = b.at + b.cue(Part::Legs, name)?;
        if end < land {
            b.at = end;
            b.cue(Part::Legs, "BOTH_INAIR1")?;
            b.at = land;
            b.wish(Wish::Velocity(Vec3::ZERO));
            b.play(Part::Legs, "BOTH_LAND1")?;
        } else {
            b.at = land;
            b.wish(Wish::Velocity(Vec3::ZERO));
            b.at = end;
        }
        b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    }
    b.put_away()?;
    Ok(b.finish("acrobatics", notes))
}

/// The saber specials, each from its style's stance: the fast style's
/// lunge (150 units/s forward, then pushed forward while it plays), the
/// strong style's jump attack (300 forward, 280 up), the medium style's
/// flip slash (150 forward, 400 up), and the back stab and back attack.
pub fn specials(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    b.mark("lunge");
    b.hold(Part::Both, "BOTH_SABERFAST_STANCE", 400.0)?;
    b.lunge()?;
    b.hold(Part::Both, "BOTH_SABERFAST_STANCE", 500.0)?;
    b.mark("jump attack");
    b.hold(Part::Both, "BOTH_SABERSLOW_STANCE", 400.0)?;
    let land = b.leap(Leap::Launch(Vec3::new(0.0, 300.0, 280.0)))?;
    b.play(Part::Both, "BOTH_FORCELEAP2_T__B_")?;
    b.at = b.at.max(land);
    b.hold(Part::Both, "BOTH_SABERSLOW_STANCE", 500.0)?;
    b.mark("flip slash");
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    let land = b.leap(Leap::Launch(Vec3::new(0.0, 150.0, 400.0)))?;
    b.play(Part::Both, "BOTH_JUMPFLIPSLASHDOWN1")?;
    b.at = b.at.max(land);
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.mark("back stab");
    b.play(Part::Both, "BOTH_A2_STABBACK1")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.mark("back attack");
    b.play(Part::Both, "BOTH_ATTACK_BACK")?;
    b.hold(Part::Both, "BOTH_STAND2", 500.0)?;
    b.put_away()?;
    Ok(b.finish("specials", notes))
}

/// Blocks and what follows them: parries held for a moment (the `P`
/// animations are two-frame poses), a knockaway (`K`), and a parried
/// attack bouncing back (`V`).
pub fn defense(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    for name in [
        "BOTH_P1_S1_T_",
        "BOTH_P1_S1_TR",
        "BOTH_P1_S1_TL",
        "BOTH_P1_S1_BR",
        "BOTH_P1_S1_BL",
    ] {
        b.mark(&name.to_ascii_lowercase());
        b.hold(Part::Both, name, 350.0)?;
        b.hold(Part::Both, "BOTH_STAND2", 250.0)?;
    }
    b.mark("knockaway");
    b.play(Part::Both, "BOTH_K1_S1_T_")?;
    b.hold(Part::Both, "BOTH_STAND2", 300.0)?;
    b.mark("bounce");
    b.play(Part::Both, "BOTH_A2_T__B_")?;
    b.play(Part::Both, "BOTH_V1_T__S1")?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    b.put_away()?;
    Ok(b.finish("defense", notes))
}

/// Force powers with the saber in the other hand (throw,
/// grip, lightning, with JA's hand-extend animations), each level 3 unless said:
/// - push and pull, torso gestures; their effect lasts as long as the
///   power's `PW_DISINT_4` debounce: 1100 ms for a push, the 400 ms hand
///   extend and 200 more for a pull;
/// - grip held;
/// - lightning held at level 2 (a single bolt, `lightning.efx`) and at 3
///   (the arc, `lightningwide.efx`), the view tipped 10° down so the bolt
///   meets the floor;
/// - a saber throw, the hand glowing while it guides the saber.
pub fn force(
    gla: &Gla,
    cfg: &AnimationConfig,
    setup: &Setup,
    notes: &mut Vec<String>,
) -> Result<Timeline, String> {
    let mut b = Builder::new(gla, cfg, setup);
    b.draw()?;
    for (name, kind, debounce) in [
        ("BOTH_FORCEPUSH", PowerKind::Push, 1100.0),
        ("BOTH_FORCEPULL", PowerKind::Pull, 600.0),
    ] {
        b.mark(&name.to_ascii_lowercase());
        b.power(kind, debounce, 3);
        b.play(Part::Torso, name)?;
        b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    }
    b.mark("both_forcegrip_hold");
    b.power(PowerKind::Grip, 1000.0, 3);
    b.hold(Part::Torso, "BOTH_FORCEGRIP_HOLD", 1000.0)?;
    b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    for level in [2, 3] {
        b.mark(&format!("lightning {level}"));
        b.view(10.0, 0.0, 300.0);
        b.hold(Part::Both, "BOTH_STAND2", 300.0)?;
        b.power(PowerKind::Lightning, 1000.0, level);
        b.hold(Part::Torso, "BOTH_FORCELIGHTNING_HOLD", 1000.0)?;
        b.view(0.0, 0.0, 300.0);
        b.hold(Part::Both, "BOTH_STAND2", 400.0)?;
    }
    b.mark("saber throw");
    b.throw(800.0, 2, setup.catches.first().copied())?;
    b.hold(Part::Torso, "BOTH_STAND2", 500.0)?;
    b.put_away()?;
    Ok(b.finish("force", notes))
}

/// A baked JA clip, the ground it covers, and when the saber turns on and
/// off ((ms, on); empty when it stays lit).
pub struct Timeline {
    pub clip: AnimationIr,
    /// Each sample's angles (ms, the legs' yaw, the torso's aim, the
    /// overrides): effects aim with the torso.
    pub angles: Vec<(f32, asset_ja::angles::Pose)>,
    /// The force powers used, for their effects.
    pub powers: Vec<Power>,
    pub ground: Ground,
    pub saber: Vec<(f32, bool)>,
    /// When each torso animation starts: (ms, name), for the trail.
    pub moves: Vec<(f32, String)>,
    /// Which sabers the timeline's animations are made for.
    pub style: Style,
    /// The right saber's throws, and when the timeline has each caught.
    pub throws: Vec<Throw>,
    pub catches: Vec<f32>,
}

/// A force power in use from `start` to `end` (ms), at a level (1 to 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Power {
    pub kind: PowerKind,
    pub start: f32,
    pub end: f32,
    pub level: u32,
}

/// The powers whose effects the cgame draws on the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerKind {
    Push,
    Pull,
    Grip,
    Lightning,
}

/// A saber style's sabers: one single saber, two (one each hand), a staff
/// (one hilt, two blades), or none (melee).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Single,
    Dual,
    Staff,
    Melee,
}

/// The sharpest steps of the look's angles between samples, for the
/// notes: the legs' yaw and each override's largest axis (degrees per
/// sample, at the timeline's ms). A step the legs' swing or a view sweep
/// cannot make is a snap.
fn angle_steps(angles: &[(f32, asset_ja::angles::Pose)]) -> String {
    let mut steps: Vec<(f32, f32, &str)> = Vec::new();
    for pair in angles.windows(2) {
        let ((_, a), (ms, b)) = (&pair[0], &pair[1]);
        let legs = asset_ja::angles::wrap_180(b.legs_yaw - a.legs_yaw).abs();
        steps.push((legs, *ms, "legs"));
        for ((name, x), (_, y)) in a.bones.iter().zip(&b.bones) {
            let step = (0..3)
                .map(|i| asset_ja::angles::wrap_180(y[i] - x[i]).abs())
                .fold(0.0, f32::max);
            steps.push((step, *ms, name));
        }
    }
    steps.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top: Vec<String> = steps
        .iter()
        .take(4)
        .map(|(step, ms, name)| format!("{name} {step:.1} at {ms:.0}"))
        .collect();
    format!("angle steps (deg per sample): {}", top.join(", "))
}

/// The first `seconds` of a clip, its keys cut there (the cut sampled).
pub fn slice(clip: &AnimationIr, seconds: f32) -> AnimationIr {
    let cut = seconds.min(clip.length);
    let tracks = clip
        .tracks
        .iter()
        .map(|track| {
            let mut positions: Vec<(f32, [f32; 3])> = track
                .positions
                .iter()
                .copied()
                .filter(|k| k.0 < cut)
                .collect();
            if let Some(p) = asset_odyssey::anim::sample_position(&track.positions, cut) {
                positions.push((cut, p));
            }
            let mut orientations: Vec<(f32, [f32; 4])> = track
                .orientations
                .iter()
                .copied()
                .filter(|k| k.0 < cut)
                .collect();
            if let Some(q) = asset_odyssey::anim::sample_orientation(&track.orientations, cut) {
                orientations.push((cut, q));
            }
            NodeTrack {
                node: track.node.clone(),
                positions,
                orientations,
            }
        })
        .collect();
    AnimationIr {
        length: cut,
        tracks,
        ..clip.clone()
    }
}

/// A clip with a node's position keys moved by `offset` (in its parent's
/// space, before the model's animation scale), the track made if missing.
pub fn shift(clip: &AnimationIr, node: &str, offset: Vec3) -> AnimationIr {
    let mut clip = clip.clone();
    let index = match clip
        .tracks
        .iter()
        .position(|t| t.node.eq_ignore_ascii_case(node))
    {
        Some(index) => index,
        None => {
            clip.tracks.push(NodeTrack {
                node: node.to_string(),
                positions: Vec::new(),
                orientations: Vec::new(),
            });
            clip.tracks.len() - 1
        }
    };
    let track = &mut clip.tracks[index];
    if track.positions.is_empty() {
        track.positions = vec![(0.0, [0.0; 3]), (clip.length, [0.0; 3])];
    }
    for key in &mut track.positions {
        key.1 = (Vec3::from_array(key.1) + offset).to_array();
    }
    clip
}

/// How clean a clip plays on a body, from `[from, to)` seconds: the feet's
/// slide while down (horizontal speed of an ankle held within 2 cm of its
/// rest height for 60 ms or more)
/// and the sharpest jolts (acceleration) of the hands, feet and head.
pub fn measure(
    body: &ModelIr,
    clip: &AnimationIr,
    from: f32,
    to: f32,
    csv: Option<&std::path::Path>,
) -> Vec<String> {
    // The nodes this body has (`trace` skips the others).
    let names: Vec<&str> = ["lfoot_g", "rfoot_g", "rhand_g", "lhand_g", "head_g"]
        .into_iter()
        .filter(|name| body.nodes.iter().any(|n| n.name.eq_ignore_ascii_case(name)))
        .collect();
    let trace: Vec<(f32, Vec<Vec3>)> = asset_ja::rig::trace(body, clip, &names)
        .into_iter()
        .filter(|(t, _)| *t >= from && *t < to)
        .collect();
    let mut notes = Vec::new();
    if let Some(path) = csv {
        let header: Vec<String> = names
            .iter()
            .flat_map(|n| ["x", "y", "z"].map(|a| format!("{n}.{a}")))
            .collect();
        let mut text = format!(
            "t,{}
",
            header.join(",")
        );
        for (t, points) in &trace {
            let cells: Vec<String> = points
                .iter()
                .flat_map(|p| p.to_array().map(|v| format!("{v:.4}")))
                .collect();
            text.push_str(&format!(
                "{t:.4},{}
",
                cells.join(",")
            ));
        }
        match std::fs::write(path, text) {
            Ok(()) => notes.push(format!("trace written to {}", path.display())),
            Err(error) => notes.push(format!("trace {}: {error}", path.display())),
        }
    }
    for (foot, name) in names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.ends_with("foot_g"))
    {
        // Down: within 2 cm of the ankle's rest height, where retargeting
        // holds a planted foot.
        let low = body
            .nodes
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case(name))
            .map_or(0.0, |n| n.world[2][3]);
        // Stretches the foot stays down at least 60 ms: a swinging foot
        // passing low is not planted.
        let down: Vec<bool> = trace.iter().map(|(_, p)| p[foot].z < low + 0.02).collect();
        let mut planted = vec![false; trace.len()];
        let mut start = 0;
        while start < trace.len() {
            if !down[start] {
                start += 1;
                continue;
            }
            let end = (start..trace.len())
                .find(|&i| !down[i])
                .unwrap_or(trace.len());
            if trace[end - 1].0 - trace[start].0 >= 0.06 {
                planted[start..end].iter_mut().for_each(|p| *p = true);
            }
            start = end;
        }
        let mut slides: Vec<(f32, f32)> = trace
            .windows(2)
            .enumerate()
            .filter(|(i, pair)| planted[*i] && planted[i + 1] && pair[1].0 > pair[0].0)
            .map(|(_, pair)| {
                let d = pair[1].1[foot] - pair[0].1[foot];
                (pair[0].0, d.truncate().length() / (pair[1].0 - pair[0].0))
            })
            .collect();
        if slides.is_empty() {
            continue;
        }
        let mean = slides.iter().map(|s| s.1).sum::<f32>() / slides.len() as f32;
        slides.sort_by(|a, b| b.1.total_cmp(&a.1));
        notes.push(format!(
            "slide {name}: down {} samples, mean {mean:.2} m/s, worst {:.2} m/s at {:.3} s",
            slides.len(),
            slides[0].1,
            slides[0].0
        ));
    }
    for (index, name) in names.iter().enumerate() {
        let mut jolts: Vec<(f32, f32)> = trace
            .windows(3)
            .filter_map(|w| {
                let (t0, t1, t2) = (w[0].0, w[1].0, w[2].0);
                let (d0, d1) = (t1 - t0, t2 - t1);
                (d0 > 1e-4 && d1 > 1e-4).then(|| {
                    let v0 = (w[1].1[index] - w[0].1[index]) / d0;
                    let v1 = (w[2].1[index] - w[1].1[index]) / d1;
                    (t1, ((v1 - v0) / ((d0 + d1) / 2.0)).length())
                })
            })
            .collect();
        jolts.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top: Vec<String> = jolts
            .iter()
            .take(3)
            .map(|(t, a)| format!("{a:.0} at {t:.3}"))
            .collect();
        notes.push(format!("jolt {name} (m/s²): {}", top.join(", ")));
    }
    notes
}
