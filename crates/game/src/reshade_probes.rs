//! The probes of kjk's ReShade support (`kjk sim probes`, group "reshade"):
//! what the overlay does to the cursor and the mouse, and when the overlay
//! cameras keep the world's depth. ReShade itself runs only on Windows, so
//! these set the overlay's state by hand.

use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use sim::probe::{Checks, check};

use super::*;

pub fn probes() -> Checks {
    vec![
        ("reshade overlay cursor", cursor_decision()),
        ("reshade overlay cursor in play", cursor_in_play()),
        ("reshade overlay mouse", mouse_dropped()),
        ("reshade depth switch", depth_switch()),
        ("reshade depth limits", depth_limits()),
    ]
}

/// The overlay opens for two frames and closes: the cursor is free while it
/// is open, play decides again the frame it closes, and goes on after.
fn cursor_decision() -> Result<String, String> {
    use OverlayCursor::{Decide, Free, Leave};
    let mut was = false;
    let frames = [false, true, true, false, false].map(|open| overlay_cursor(open, &mut was));
    check(
        frames == [Leave, Free, Free, Decide, Leave],
        format!("{frames:?}"),
    )
}

/// The cursor after three frames of `grab_cursor`, with ReShade's overlay
/// closed, open and closed again. `screen` and `talking` are whether a
/// screen is open and whether a conversation runs in each frame.
fn cursor_after(screen: [bool; 3], talking: [bool; 3]) -> Option<(CursorGrabMode, bool)> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::scene::Headless(false))
        .init_resource::<crate::swallow::Swallowed>()
        .init_resource::<crate::ui_view::UiOpen>()
        .init_resource::<crate::dialogue_view::ReplyInput>()
        .init_resource::<Overlay>()
        .add_systems(Update, crate::sim_host::grab_cursor);
    let window = app
        .world_mut()
        .spawn((
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    for (frame, open) in [false, true, false].into_iter().enumerate() {
        let world = app.world_mut();
        world.resource_mut::<Overlay>().0 = open;
        world.resource_mut::<crate::ui_view::UiOpen>().0 = screen[frame];
        world
            .resource_mut::<crate::dialogue_view::ReplyInput>()
            .active = talking[frame];
        app.update();
    }
    let cursor = app.world().get::<CursorOptions>(window)?;
    Some((cursor.grab_mode, cursor.visible))
}

/// Overlay closed, open, closed. In play the cursor is held and hidden
/// again after. A screen that opened while the overlay was open, or a
/// conversation, leaves it free and shown.
fn cursor_in_play() -> Result<String, String> {
    let held = (CursorGrabMode::Locked, false);
    let free = (CursorGrabMode::None, true);
    let cases = [
        ("play", [false; 3], [false; 3], held),
        (
            "a screen opened meanwhile",
            [false, true, true],
            [false; 3],
            free,
        ),
        ("a conversation", [false; 3], [true; 3], free),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, screen, talking, wanted)| {
            let got = cursor_after(*screen, *talking);
            (got != Some(*wanted)).then(|| format!("{name}: {got:?}, not {wanted:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        if wrong.is_empty() {
            "the cursor is held after the overlay in play, and free after it with a screen \
             open or in a conversation"
                .into()
        } else {
            wrong.join("; ")
        },
    )
}

/// The frame's mouse input after `drop_mouse` with the overlay `open`: the
/// motion, the scroll, whether the left button is held, and the number of
/// mouse messages left.
fn mouse_after(open: bool) -> (Vec2, Vec2, bool, usize) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Overlay(open))
        .insert_resource(AccumulatedMouseMotion {
            delta: Vec2::new(3.0, 4.0),
        })
        .insert_resource(AccumulatedMouseScroll {
            delta: Vec2::new(0.0, 1.0),
            ..default()
        })
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .add_message::<CursorMoved>()
        .add_systems(Update, drop_mouse.run_if(overlay_open));
    let world = app.world_mut();
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.write_message(MouseMotion {
        delta: Vec2::new(3.0, 4.0),
    });
    world.write_message(MouseWheel {
        unit: bevy::input::mouse::MouseScrollUnit::Line,
        x: 0.0,
        y: 1.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    world.write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: bevy::input::ButtonState::Pressed,
        window: Entity::PLACEHOLDER,
    });
    world.write_message(CursorMoved {
        window: Entity::PLACEHOLDER,
        position: Vec2::ONE,
        delta: None,
    });
    world.run_schedule(Update);
    let world = app.world();
    let messages = world.resource::<Messages<MouseMotion>>().len()
        + world.resource::<Messages<MouseWheel>>().len()
        + world.resource::<Messages<MouseButtonInput>>().len()
        + world.resource::<Messages<CursorMoved>>().len();
    (
        world.resource::<AccumulatedMouseMotion>().delta,
        world.resource::<AccumulatedMouseScroll>().delta,
        world
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left),
        messages,
    )
}

/// The overlay open, the frame's mouse input is gone. Closed, it is all
/// still there.
fn mouse_dropped() -> Result<String, String> {
    let (open, closed) = (mouse_after(true), mouse_after(false));
    check(
        open == (Vec2::ZERO, Vec2::ZERO, false, 0)
            && closed == (Vec2::new(3.0, 4.0), Vec2::new(0.0, 1.0), true, 4),
        format!(
            "(motion, scroll, left held, messages) with the overlay open {open:?}, closed \
             {closed:?}"
        ),
    )
}

/// The overlay cameras keep the world's depth when ReShade is loaded, and
/// `KJK_RESHADE_DEPTH` turns that on or off whatever ReShade does.
fn depth_switch() -> Result<String, String> {
    let cases = [
        (None, false, false),
        (None, true, true),
        (Some("1"), false, true),
        (Some("0"), true, false),
        (Some("yes"), true, true),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(env, loaded, wanted)| keeps_depth_choice(*env, *loaded) != *wanted)
        .map(|case| format!("{case:?}"))
        .collect();
    check(
        wrong.is_empty(),
        format!("cases (KJK_RESHADE_DEPTH, loaded, kept) that are wrong: {wrong:?}"),
    )
}

/// Depth effects work under Off, TAA and DLAA. They do not under MSAA 2 or
/// 4, nor when the world is drawn smaller than the window.
fn depth_limits() -> Result<String, String> {
    use crate::temporal::Upscaler;
    let cases = [
        (Antialias::Msaa(1), true),
        (Antialias::Taa, true),
        (Antialias::Upscale(Upscaler::Dlss, Scale::Full), true),
        (Antialias::Msaa(2), false),
        (Antialias::Msaa(4), false),
        (Antialias::Upscale(Upscaler::Taa, Scale::Performance), false),
        (Antialias::Upscale(Upscaler::Dlss, Scale::Quality), false),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(used, works)| depth_limit(*used).is_none() != *works)
        .map(|case| format!("{case:?}"))
        .collect();
    check(
        wrong.is_empty(),
        format!("choices (anti-aliasing, depth works) that are wrong: {wrong:?}"),
    )
}
