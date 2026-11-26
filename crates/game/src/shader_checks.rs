//! Checks that kjk's own shaders never read past a list or run a loop away.
//!
//! Bevy compiles shaders without runtime checks. A shader that reads a list
//! at an index past its end, or a loop whose count is wrong, then does what
//! the GPU does. Metal forgives it. NVIDIA's driver under Vulkan may lose the
//! render device (issues 431, 432, 433, 435 and 473). So every index that a
//! shader reads from a buffer is clamped to its list's length in the shader
//! itself, and every loop whose count is read from a buffer is clamped too.
//! The probes here read the shaders' source and fail when one is not.
//!
//! Every kjk shader is also loaded through `wgsl`, which can turn on wgpu's
//! runtime checks for it (`checked`). wgpu then clamps every index into an
//! array in the shader, and bounds every loop. Bevy's own shaders stay
//! unchecked.

use bevy::shader::{Shader, ValidateShader};

/// Whether kjk's shaders are compiled with wgpu's runtime checks.
/// `KJK_CHECKED_SHADERS=1` (or `on`) turns them on and `0` (or `off`)
/// turns them off. Without it they are `CHECKED_BY_DEFAULT`.
pub fn checked() -> bool {
    static CHECKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECKED.get_or_init(|| match std::env::var("KJK_CHECKED_SHADERS").as_deref() {
        Ok("1" | "on") => true,
        Ok("0" | "off") => false,
        _ => CHECKED_BY_DEFAULT,
    })
}

/// Whether kjk's shaders are checked when `KJK_CHECKED_SHADERS` is not set.
pub const CHECKED_BY_DEFAULT: bool = false;

/// A kjk shader from its WGSL source, checked when `checked` says so. It
/// takes the place of `Shader::from_wgsl` in `load_internal_asset!`.
pub fn wgsl(source: impl Into<std::borrow::Cow<'static, str>>, path: impl Into<String>) -> Shader {
    let mut shader = Shader::from_wgsl(source, path);
    if checked() {
        shader.validate_shader = ValidateShader::Enabled;
    }
    shader
}

/// Every kjk shader, by its file name.
pub const SHADERS: &[(&str, &str)] = &[
    (
        "ambient_occlusion.wgsl",
        include_str!("ambient_occlusion.wgsl"),
    ),
    ("dangly.wgsl", include_str!("dangly.wgsl")),
    ("effects_quality.wgsl", include_str!("effects_quality.wgsl")),
    ("glow_shell.wgsl", include_str!("glow_shell.wgsl")),
    ("grass.wgsl", include_str!("grass.wgsl")),
    ("heat_haze.wgsl", include_str!("heat_haze.wgsl")),
    ("lighting.wgsl", include_str!("lighting.wgsl")),
    ("linear_frame.wgsl", include_str!("linear_frame.wgsl")),
    ("motion_debug.wgsl", include_str!("motion_debug.wgsl")),
    ("nowhere.wgsl", include_str!("nowhere.wgsl")),
    ("odyssey.wgsl", include_str!("odyssey.wgsl")),
    ("odyssey_prepass.wgsl", include_str!("odyssey_prepass.wgsl")),
    ("particles.wgsl", include_str!("particles.wgsl")),
    ("refraction.wgsl", include_str!("refraction.wgsl")),
    ("ring_distort.wgsl", include_str!("ring_distort.wgsl")),
    ("rolloff.wgsl", include_str!("rolloff.wgsl")),
    ("screen_copy.wgsl", include_str!("screen_copy.wgsl")),
    ("shadow_tiles.wgsl", include_str!("shadow_tiles.wgsl")),
    ("shell.wgsl", include_str!("shell.wgsl")),
    ("skin_once.wgsl", include_str!("skin_once.wgsl")),
    ("speed_blur.wgsl", include_str!("speed_blur.wgsl")),
    ("sway.wgsl", include_str!("sway.wgsl")),
    ("ui.wgsl", include_str!("ui.wgsl")),
    ("ui_composite.wgsl", include_str!("ui_composite.wgsl")),
    ("upscale.wgsl", include_str!("upscale.wgsl")),
    ("sharpen.wgsl", include_str!("sharpen.wgsl")),
    ("video_effect.wgsl", include_str!("video_effect.wgsl")),
];

/// The lists in buffers that a shader reads at an index it got from a
/// buffer or from the draw. Each read must clamp its index with `min(`.
const CLAMPED: &[(&str, &[&str])] = &[
    (
        "odyssey.wgsl",
        &[
            "shadows.casters[",
            "shadows.capsules[",
            "shadows.tiles[",
            "shadows.near_boxes[",
            "boxes.boxes[",
            "boxes.tiles[",
            "lights.lights[",
            "animated[",
            "ray_numbers[",
            "probe_list.entries[",
            "probe_list.room_lights[",
        ],
    ),
    ("odyssey_prepass.wgsl", &["animated["]),
    ("shadow_tiles.wgsl", &["joints[", "draws[", "sway["]),
    ("particles.wgsl", &["particles["]),
    ("dangly.wgsl", &["sway["]),
    ("shell.wgsl", &["sway["]),
    ("glow_shell.wgsl", &["sway["]),
];

/// The reads of `CLAMPED` whose index does not start with `min(`, as
/// "file:line: text".
pub fn unclamped_reads() -> Vec<String> {
    let mut found = Vec::new();
    for (file, lists) in CLAMPED {
        let Some((_, source)) = SHADERS.iter().find(|(name, _)| name == file) else {
            found.push(format!("{file}: not among the shaders"));
            continue;
        };
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for list in *lists {
                let mut rest = code;
                while let Some(at) = rest.find(list) {
                    // A longer name that ends in this one (`near_boxes[` in
                    // `shadows.near_boxes[`) is its own entry.
                    let before = rest[..at].chars().last();
                    let after = &rest[at + list.len()..];
                    let whole =
                        before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '.'));
                    if whole && !after.starts_with("min(") {
                        found.push(format!("{file}:{}: {}", number + 1, line.trim()));
                    }
                    rest = after;
                }
            }
        }
    }
    found
}

/// The `for` loops whose bound reads a value from a buffer or a struct (a
/// name with a field, such as `near.count`) without clamping it with
/// `min(`, as "file:line: text". A bound that is a constant, a local or a
/// call to a function that clamps (`light_count()`) passes.
pub fn unclamped_loops() -> Vec<String> {
    let mut found = Vec::new();
    for (file, source) in SHADERS {
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            let Some(head) = code.strip_prefix("for (") else {
                continue;
            };
            let condition = head.split(';').nth(1).unwrap_or("").trim();
            let bound = condition
                .split_once("<=")
                .or_else(|| condition.split_once('<'))
                .map_or("", |(_, bound)| bound.trim());
            if bound.contains('.') && !bound.starts_with("min(") {
                found.push(format!("{file}:{}: {code}", number + 1));
            }
        }
    }
    found
}

/// The value of `const NAME: u32 = N;` (or `Nu`) in `source`.
fn wgsl_const(source: &str, name: &str) -> Option<usize> {
    let wanted = format!("const {name}: u32 = ");
    source.lines().find_map(|line| {
        let value = line.trim().strip_prefix(&wanted)?;
        value
            .trim_end_matches(';')
            .trim_end_matches('u')
            .parse()
            .ok()
    })
}

/// The length of the field `field: array<_, N>` in `source`.
fn wgsl_array(source: &str, field: &str) -> Option<usize> {
    let wanted = format!("{field}: array<");
    source.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(&wanted)?;
        let (_, length) = rest.rsplit_once(", ")?;
        length
            .trim_end_matches(">,")
            .trim_end_matches('>')
            .parse()
            .ok()
    })
}

/// The lengths in `odyssey.wgsl` that differ from the CPU's, as
/// "name: shader N, CPU M". A shader that clamps to a longer length than
/// the CPU wrote would still read past what the CPU meant.
pub fn mismatched_lengths() -> Vec<String> {
    use crate::shadows;
    let source = include_str!("odyssey.wgsl");
    let wanted: [(&str, Option<usize>, usize); 17] = [
        ("CASTERS", wgsl_const(source, "CASTERS"), shadows::CASTERS),
        (
            "NEAR_BOXES",
            wgsl_const(source, "NEAR_BOXES"),
            shadows::NEAR_BOXES,
        ),
        (
            "CAPSULES",
            wgsl_const(source, "CAPSULES"),
            shadows::CAPSULES,
        ),
        ("TILES", wgsl_const(source, "TILES"), shadows::TILES),
        ("BOXES", wgsl_const(source, "BOXES"), shadows::BOXES),
        (
            "KEPT_TILES",
            wgsl_const(source, "KEPT_TILES"),
            shadows::KEPT_TILES,
        ),
        (
            "KEPT_LIGHTS",
            wgsl_const(source, "KEPT_LIGHTS"),
            shadows::KEPT_LIGHTS,
        ),
        (
            "MAX_LIGHTS",
            wgsl_const(source, "MAX_LIGHTS"),
            crate::material::MAX_LIGHTS,
        ),
        (
            "ANIMATED_SLOTS",
            wgsl_const(source, "ANIMATED_SLOTS"),
            crate::material::ANIMATED_SLOTS,
        ),
        (
            "PROBE_SLOTS",
            wgsl_const(source, "PROBE_SLOTS"),
            crate::light_probes::PROBE_SLOTS,
        ),
        ("casters", wgsl_array(source, "casters"), shadows::CASTERS),
        (
            "capsules",
            wgsl_array(source, "capsules"),
            shadows::CAPSULES,
        ),
        (
            "near_boxes",
            wgsl_array(source, "near_boxes"),
            shadows::NEAR_BOXES,
        ),
        ("boxes", wgsl_array(source, "boxes"), shadows::BOXES),
        (
            "lights",
            wgsl_array(source, "lights"),
            crate::material::MAX_LIGHTS,
        ),
        (
            "entries",
            wgsl_array(source, "entries"),
            crate::light_probes::PROBE_SLOTS,
        ),
        (
            "room_lights",
            wgsl_array(source, "room_lights"),
            crate::light_probes::PROBE_SLOTS,
        ),
    ];
    wanted
        .into_iter()
        .filter(|(_, shader, cpu)| *shader != Some(*cpu))
        .map(|(name, shader, cpu)| format!("{name}: shader {shader:?}, CPU {cpu}"))
        .collect()
}

/// The probes of the shaders' source (`kjk sim probes`, group "shader
/// bounds").
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let listed = |found: Vec<String>| {
        let detail = if found.is_empty() {
            "none".to_string()
        } else {
            found.join("; ")
        };
        check(found.is_empty(), detail)
    };
    vec![
        (
            "every index read from a buffer is clamped to its list",
            listed(unclamped_reads()),
        ),
        (
            "every loop whose count is read from a buffer is clamped",
            listed(unclamped_loops()),
        ),
        (
            "the shaders' list lengths are the CPU's",
            listed(mismatched_lengths()),
        ),
    ]
}
