//! Jedi Academy's ambient effects placed in an area: smoke, steam, sparks,
//! fire, dust and the like, from the effect files of the player's own Jedi
//! Academy (`effects/**/*.efx`). Play plays each one again and again where
//! it stands, as Jedi Academy's `fx_runner` entity does
//! (`game::ambient_fx`). The project names the effect and never holds a
//! copy of it.
//!
//! ```toml
//! [[effect]]
//! name = "effect-1"
//! effect = "effects/env/fire.efx"
//! at = [10.5, 2.0, 0.0]
//! yaw = 0.0
//! pitch = 90.0
//! every = 0.2
//! ```
//!
//! `at` is metres in KOTOR's space (Z up). The effect points the way `yaw`
//! (degrees from +X, turning left) and `pitch` (degrees over the level)
//! give. Jedi Academy's ambient effects are made to point up, so a new one
//! starts at a pitch of 90. `every` is how many seconds pass between two
//! plays.

/// The fewest and the most seconds between two plays of an effect.
pub const EVERY_LEAST: f32 = 0.05;
pub const EVERY_MOST: f32 = 60.0;
/// The seconds between two plays of a new effect: Jedi Academy's
/// `fx_runner` waits 200 milliseconds when its map sets no `delay`.
pub const EVERY_START: f32 = 0.2;
/// A new effect's pitch: straight up.
pub const PITCH_START: f32 = 90.0;

/// An ambient effect placed in the area.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientEffect {
    pub name: String,
    /// The effect file's path in Jedi Academy's archives, lower case, with
    /// `/` between folders: `effects/env/fire.efx`.
    pub effect: String,
    /// Where it plays (metres, game space).
    pub at: [f32; 3],
    /// Degrees from +X, turning left.
    pub yaw: f32,
    /// Degrees over the level: 90 points straight up, -90 straight down.
    pub pitch: f32,
    /// Seconds between two plays.
    pub every: f32,
}

impl AmbientEffect {
    /// A new effect of `effect` at `at`, named `name`, pointing up.
    pub fn new(name: &str, effect: &str, at: [f32; 3]) -> AmbientEffect {
        AmbientEffect {
            name: name.to_string(),
            effect: effect_path(effect),
            at,
            yaw: 0.0,
            pitch: PITCH_START,
            every: EVERY_START,
        }
    }

    /// The way it points, of length 1 (game space).
    pub fn forward(&self) -> [f32; 3] {
        let (yaw_sin, yaw_cos) = self.yaw.to_radians().sin_cos();
        let (pitch_sin, pitch_cos) = self.pitch.to_radians().sin_cos();
        [pitch_cos * yaw_cos, pitch_cos * yaw_sin, pitch_sin]
    }

    /// The name the game plays it by: its path without `effects/` and
    /// `.efx` (`env/fire`).
    pub fn played_name(&self) -> String {
        played_name(&self.effect)
    }

    /// Its `[[effect]]` table, its numbers as short as they read back the
    /// same.
    pub fn table(&self) -> toml_edit::Table {
        use crate::lights::{float, triple_value};
        let mut table = toml_edit::Table::new();
        table.insert("name", toml_edit::value(self.name.as_str()));
        table.insert("effect", toml_edit::value(self.effect.as_str()));
        table.insert("at", triple_value(self.at));
        table.insert("yaw", float(self.yaw));
        table.insert("pitch", float(self.pitch));
        table.insert("every", float(self.every));
        table
    }

    /// An `[[effect]]` table read back.
    pub fn read(table: &toml_edit::Table) -> Result<AmbientEffect, String> {
        use crate::lights::{number, text, triple};
        let effect = AmbientEffect {
            name: text(table.get("name")),
            effect: effect_path(&text(table.get("effect"))),
            at: triple(table.get("at")).ok_or("an effect's at is not three numbers")?,
            yaw: number(table.get("yaw")).unwrap_or(0.0),
            pitch: number(table.get("pitch")).unwrap_or(PITCH_START),
            every: number(table.get("every")).unwrap_or(EVERY_START),
        };
        problem(&effect)?;
        Ok(effect)
    }
}

/// An effect's path as the project keeps it: lower case, `/` between
/// folders, under `effects/` and ending `.efx` (`env/fire` and
/// `effects\env\fire.efx` are both `effects/env/fire.efx`).
pub fn effect_path(effect: &str) -> String {
    let path = effect.trim().replace('\\', "/").to_ascii_lowercase();
    let path = path.trim_start_matches('/');
    let path = if path.starts_with("effects/") {
        path.to_string()
    } else {
        format!("effects/{path}")
    };
    if path.ends_with(".efx") {
        path
    } else {
        format!("{path}.efx")
    }
}

/// The name the game plays an effect path by (`AmbientEffect::played_name`).
pub fn played_name(effect: &str) -> String {
    let path = effect_path(effect);
    path.trim_start_matches("effects/")
        .trim_end_matches(".efx")
        .to_string()
}

/// Why an effect cannot be kept as it is, if it cannot.
pub fn problem(effect: &AmbientEffect) -> Result<(), String> {
    if effect.name.trim().is_empty() {
        return Err("an effect needs a name".into());
    }
    if effect.effect.len() <= "effects/.efx".len() {
        return Err("an effect needs an effect file".into());
    }
    if !(effect
        .at
        .iter()
        .chain([&effect.yaw, &effect.pitch, &effect.every]))
    .all(|v| v.is_finite())
    {
        return Err("an effect's numbers must be finite".into());
    }
    if !(-90.0..=90.0).contains(&effect.pitch) {
        return Err("an effect's pitch is from -90 to 90 degrees".into());
    }
    if !(EVERY_LEAST..=EVERY_MOST).contains(&effect.every) {
        return Err(format!(
            "an effect plays every {EVERY_LEAST} to {EVERY_MOST} seconds"
        ));
    }
    Ok(())
}
