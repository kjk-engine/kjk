//! Small rules the JA view plays by: a blade lighting and going out, how
//! long a saber move's trail lingers, which animations blend slower or
//! count as deaths, the faster and slower saber styles' transitions, and
//! how a held move stretches with its speed.

/// A blade's length as it lights or goes out, one client frame at a time.
///
/// It leaves rest with a one-unit head start and then moves by a hundredth
/// of the time since it set off each frame (at least 0.2), so the higher
/// the frame rate, the slower it lights (QUIRKS.md Q-0005).
#[derive(Debug, Clone, Copy, Default)]
pub struct Ignition {
    pub length: f32,
    /// When the blade last left rest (fully out or fully lit).
    set_off_ms: f32,
}

impl Ignition {
    /// One frame at `now_ms` toward `target` (0 or `full`).
    pub fn frame(&mut self, now_ms: f32, target: f32, full: f32) {
        if self.length == target {
            return;
        }
        if self.length == 0.0 || self.length == full {
            self.set_off_ms = now_ms;
            self.length += if self.length == 0.0 { 1.0 } else { -1.0 };
        }
        let step = ((now_ms - self.set_off_ms) * 0.01).max(0.2);
        self.length = if self.length < target {
            (self.length + step).min(target)
        } else {
            (self.length - step).max(target)
        };
    }
}

/// A blade's length over `length_ms` of frames `frame_ms` apart, as
/// (seconds, length): it is lit from the latest `toggles` entry (time in
/// ms, on) whose time has come, and out before any.
pub fn blade_lengths(
    toggles: &[(f32, bool)],
    max: f32,
    length_ms: f32,
    frame_ms: f32,
) -> Vec<(f32, f32)> {
    let mut blade = Ignition::default();
    let frames = (length_ms / frame_ms).ceil() as usize;
    (0..=frames)
        .map(|k| {
            let t = (k as f32 * frame_ms).min(length_ms);
            let lit = toggles
                .iter()
                .rev()
                .find(|toggle| toggle.0 <= t)
                .is_some_and(|toggle| toggle.1);
            blade.frame(t, if lit { max } else { 0.0 }, max);
            (t / 1000.0, blade.length)
        })
        .collect()
}

/// How long (ms) a saber move's trail lingers behind the blade: the
/// special moves their own time, the attacks, starts and returns of every
/// style a little longer than the transitions, bounces, deflections,
/// knockaways, parries and broken moves, anything else none. Each is a
/// fifth of the time the game's saber move data gives it.
pub fn saber_trail_ms(animation: &str) -> f32 {
    let name = animation.to_ascii_uppercase();
    if let Some(&(_, trail)) = SPECIAL_TRAILS.iter().find(|(special, _)| *special == name) {
        return trail / 5.0;
    }
    let Some(rest) = name.strip_prefix("BOTH_") else {
        return 0.0;
    };
    // A styled move reads `<kind><style>_…` with at least three parts; the
    // two-part draws and put-aways (`S1_S7`) have no trail.
    let shaped = match rest.as_bytes() {
        [_, style, b'_', ..] => style.is_ascii_digit() && rest.split('_').count() >= 3,
        _ => false,
    };
    if !shaped {
        return 0.0;
    }
    match rest.as_bytes()[0] {
        b'A' | b'S' | b'R' => 200.0 / 5.0,
        b'T' | b'B' | b'D' | b'K' | b'P' | b'V' | b'H' => 150.0 / 5.0,
        _ => 0.0,
    }
}

/// The special saber moves and their trail times (ms) in the game's saber
/// move data, sorted by name.
pub const SPECIAL_TRAILS: &[(&str, f32)] = &[
    ("BOTH_A1_SPECIAL", 2000.0),
    ("BOTH_A2_SPECIAL", 2000.0),
    ("BOTH_A2_STABBACK1", 200.0),
    ("BOTH_A3_SPECIAL", 2000.0),
    ("BOTH_A6_FB", 200.0),
    ("BOTH_A6_LR", 200.0),
    ("BOTH_A6_SABERPROTECT", 500.0),
    ("BOTH_A7_HILT", 200.0),
    ("BOTH_A7_SOULCAL", 500.0),
    ("BOTH_ALORA_SPIN_SLASH", 200.0),
    ("BOTH_ARIAL_LEFT", 200.0),
    ("BOTH_ARIAL_RIGHT", 200.0),
    ("BOTH_ATTACK_BACK", 200.0),
    ("BOTH_BUTTERFLY_FL1", 200.0),
    ("BOTH_BUTTERFLY_FR1", 200.0),
    ("BOTH_BUTTERFLY_LEFT", 200.0),
    ("BOTH_BUTTERFLY_RIGHT", 200.0),
    ("BOTH_CARTWHEEL_LEFT", 200.0),
    ("BOTH_CARTWHEEL_RIGHT", 200.0),
    ("BOTH_CROUCHATTACKBACK1", 200.0),
    ("BOTH_FLIP_ATTACK7", 200.0),
    ("BOTH_FORCELEAP2_T__B_", 200.0),
    ("BOTH_FORCELONGLEAP_ATTACK", 200.0),
    ("BOTH_JUMPATTACK6", 200.0),
    ("BOTH_JUMPATTACK7", 200.0),
    ("BOTH_JUMPFLIPSLASHDOWN1", 200.0),
    ("BOTH_JUMPFLIPSTABDOWN", 200.0),
    ("BOTH_LUNGE2_B__T_", 200.0),
    ("BOTH_PULL_IMPALE_STAB", 200.0),
    ("BOTH_PULL_IMPALE_SWING", 200.0),
    ("BOTH_ROLL_STAB", 200.0),
    ("BOTH_SPINATTACK6", 200.0),
    ("BOTH_SPINATTACK7", 200.0),
    ("BOTH_STABDOWN", 200.0),
    ("BOTH_STABDOWN_DUAL", 200.0),
    ("BOTH_STABDOWN_STAFF", 200.0),
    ("BOTH_VS_ATL_S", 200.0),
    ("BOTH_VS_ATR_S", 200.0),
    ("BOTH_VT_ATL_S", 200.0),
    ("BOTH_VT_ATR_S", 200.0),
];

/// The flips, wall runs, butterflies, arials and cartwheels that
/// multiplayer's client blends into and out of over 200 ms instead of 100.
pub const FLIPPING: &[&str] = &[
    "BOTH_A7_SOULCAL",
    "BOTH_ARIAL_F1",
    "BOTH_ARIAL_LEFT",
    "BOTH_ARIAL_RIGHT",
    "BOTH_BUTTERFLY_FL1",
    "BOTH_BUTTERFLY_FR1",
    "BOTH_BUTTERFLY_LEFT",
    "BOTH_BUTTERFLY_RIGHT",
    "BOTH_CARTWHEEL_LEFT",
    "BOTH_CARTWHEEL_RIGHT",
    "BOTH_FLIP_ATTACK7",
    "BOTH_FLIP_B",
    "BOTH_FLIP_BACK1",
    "BOTH_FLIP_BACK2",
    "BOTH_FLIP_BACK3",
    "BOTH_FLIP_F",
    "BOTH_FLIP_L",
    "BOTH_FLIP_R",
    "BOTH_FORCEWALLRUNFLIP_ALT",
    "BOTH_FORCEWALLRUNFLIP_END",
    "BOTH_JUMPATTACK6",
    "BOTH_JUMPATTACK7",
    "BOTH_JUMPFLIPSLASHDOWN1",
    "BOTH_JUMPFLIPSTABDOWN",
    "BOTH_WALL_FLIP_BACK1",
    "BOTH_WALL_FLIP_LEFT",
    "BOTH_WALL_FLIP_RIGHT",
    "BOTH_WALL_RUN_LEFT",
    "BOTH_WALL_RUN_LEFT_FLIP",
    "BOTH_WALL_RUN_LEFT_STOP",
    "BOTH_WALL_RUN_RIGHT",
    "BOTH_WALL_RUN_RIGHT_FLIP",
    "BOTH_WALL_RUN_RIGHT_STOP",
];

/// The deaths that are not named `BOTH_DEATH…` or `BOTH_DEAD…`: lying,
/// stumbling and falling ones and their dead poses.
const OTHER_DEATHS: [&str; 7] = [
    "BOTH_FALLDEATH1",
    "BOTH_FALLDEATH1INAIR",
    "BOTH_FALLDEATH1LAND",
    "BOTH_LYINGDEAD1",
    "BOTH_LYINGDEATH1",
    "BOTH_STUMBLEDEAD1",
    "BOTH_STUMBLEDEATH1",
];

/// Whether an animation is a death or a dead pose (any case).
pub fn is_death(animation: &str) -> bool {
    let name = animation.to_ascii_uppercase();
    name.starts_with("BOTH_DEATH")
        || name.starts_with("BOTH_DEAD")
        || OTHER_DEATHS.contains(&&*name)
}

fn flipping(animation: &str) -> bool {
    FLIPPING
        .iter()
        .any(|flip| flip.eq_ignore_ascii_case(animation))
}

/// How long (ms) a part blends from `old` into `new`: 200 when either
/// flips, else 100; with `quirks`, a death on either side cuts at once
/// (QUIRKS.md Q-0009).
pub fn blend_ms(old: Option<&str>, new: &str, quirks: bool) -> f32 {
    let either = |rule: fn(&str) -> bool| rule(new) || old.is_some_and(rule);
    if quirks && either(is_death) {
        0.0
    } else if either(flipping) {
        200.0
    } else {
        100.0
    }
}

/// The playback speed of a saber animation for a style level: the fast
/// style's own transitions run half again as fast, the strong style's at
/// three quarters; everything else as authored.
pub fn saber_anim_speed(style_level: u32, animation: &str) -> f32 {
    let name = animation.to_ascii_uppercase();
    match style_level {
        1 if name.starts_with("BOTH_T1_") => 1.5,
        3 if name.starts_with("BOTH_T3_") => 0.75,
        _ => 1.0,
    }
}

/// How long (ms) a move is held at `speed`: the whole hold plus the whole
/// part of what the speed takes from it, so a faster move holds shorter
/// and a slower one longer (QUIRKS.md Q-0008).
pub fn move_hold_ms(hold_ms: f32, speed: f32) -> f32 {
    let hold = hold_ms.trunc();
    hold + (hold - hold * speed).trunc()
}
