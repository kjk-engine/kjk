//! The console's overlay without a window: the key opens and closes it and
//! is never typed, keys edit the line at its caret, the history and a paste
//! of three lines work, Tab offers choices and Enter takes one, and a
//! movement key does nothing in play while the console is open.

use bevy::ecs::system::RunSystemOnce;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use super::{Asked, ConsoleLog, Overlay, Press, Scripted, capture};

/// A world with what `capture` reads: the frame's keys, the default key
/// map, and a run that may use the console when `scripted`.
fn world(scripted: bool) -> World {
    let mut world = World::new();
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<ButtonInput<KeyCode>>();
    world.init_resource::<Overlay>();
    world.init_resource::<ConsoleLog>();
    world.init_resource::<crate::swallow::Swallowed>();
    world.init_resource::<Time>();
    world.insert_resource(Scripted(scripted));
    world.insert_resource(crate::settings::KeyMap::defaults().clone());
    world
}

/// One frame with `code` pressed, typing `text`; whether the key still
/// reads as held for play afterwards.
fn frame(world: &mut World, code: KeyCode, text: &str) -> bool {
    world.resource_mut::<ButtonInput<KeyCode>>().press(code);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(KeyboardInput {
            key_code: code,
            logical_key: Key::Character(text.into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    if let Err(error) = world.run_system_once(capture) {
        warn!("the console's capture did not run: {error}");
    }
    let held = world.resource::<ButtonInput<KeyCode>>().pressed(code);
    let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
    keys.release(code);
    keys.clear();
    world.resource_mut::<Messages<KeyboardInput>>().update();
    held
}

fn state(world: &World) -> (bool, String) {
    let overlay = world.resource::<Overlay>();
    (overlay.open, overlay.edit.as_str())
}

/// Presses in turn; every line they asked to run.
fn pressed(overlay: &mut Overlay, presses: Vec<Press>) -> Vec<String> {
    presses
        .into_iter()
        .flat_map(|press| overlay.press(press, 0).lines)
        .collect()
}

fn text(words: &str) -> Press {
    Press::Text(words.into())
}

pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    vec![
        ("the key opens and closes the console and is never typed", {
            let mut world = world(true);
            frame(&mut world, KeyCode::Backquote, "`");
            let opened = state(&world);
            frame(&mut world, KeyCode::KeyA, "a");
            frame(&mut world, KeyCode::Backquote, "`");
            let closed = state(&world);
            let mut off = self::world(false);
            frame(&mut off, KeyCode::Backquote, "`");
            let refused = state(&off);
            check(
                opened == (true, String::new())
                    && closed == (false, "a".into())
                    && refused == (false, String::new()),
                format!("opened {opened:?}, closed {closed:?}, with the option off {refused:?}"),
            )
        }),
        (
            "a movement key does nothing in play while the console is open",
            {
                let mut world = world(true);
                let walked = frame(&mut world, KeyCode::KeyW, "w");
                frame(&mut world, KeyCode::Backquote, "`");
                let held = frame(&mut world, KeyCode::KeyW, "w");
                let typed = state(&world).1;
                check(
                    walked && !held && typed == "w",
                    format!("held with the console closed {walked}, open {held}, typed {typed:?}"),
                )
            },
        ),
        ("keys edit the line at its caret", {
            let mut overlay = Overlay::default();
            pressed(
                &mut overlay,
                vec![
                    text("gvecredits 50"),
                    Press::Home,
                    Press::Right,
                    text("i"),
                    Press::End,
                    text("0x"),
                    Press::Backspace,
                    Press::Left,
                    Press::Left,
                    Press::Delete,
                ],
            );
            let line = overlay.edit.as_str();
            check(line == "givecredits 50", format!("the line {line:?}"))
        }),
        (
            "Enter runs the line, and Up and Down go through the history",
            {
                let mut overlay = Overlay::default();
                let ran = pressed(
                    &mut overlay,
                    vec![
                        text("help"),
                        Press::Enter,
                        text("god"),
                        Press::Enter,
                        text("god"),
                        Press::Enter,
                        Press::Enter,
                        text("half"),
                    ],
                );
                let mut seen = Vec::new();
                for press in [Press::Up, Press::Up, Press::Up, Press::Down, Press::Down] {
                    overlay.press(press, 0);
                    seen.push(overlay.edit.as_str());
                }
                check(
                    ran == ["help", "god", "god"]
                        && overlay.history == ["help", "god"]
                        && seen == ["god", "help", "help", "god", "half"],
                    format!("ran {ran:?}, kept {:?}, browsed {seen:?}", overlay.history),
                )
            },
        ),
        ("a paste of three lines runs each line", {
            let mut overlay = Overlay::default();
            let one = overlay.press(Press::Paste("giveitem".into()), 0);
            let typed = overlay.edit.as_str();
            overlay.edit.clear();
            let three = overlay.press(Press::Paste("god\r\nheal\n\nwhereami\n".into()), 0);
            check(
                one == Asked::default()
                    && typed == "giveitem"
                    && three.lines == ["god", "heal", "whereami"],
                format!("one line typed {typed:?}, three ran {:?}", three.lines),
            )
        }),
        ("Tab offers choices and Enter takes the one chosen", {
            let facts = commands::Facts {
                loaded: true,
                items: Some(vec!["g_w_blstrpstl001".into(), "g_w_lghtsbr01".into()]),
                ..commands::Facts::default()
            };
            let mut overlay = Overlay::default();
            overlay.press(text("givecredi"), 0);
            overlay.press(Press::Tab, 0);
            overlay.complete(&facts);
            let alone = overlay.edit.as_str();
            overlay.edit.clear();
            overlay.press(text("giveitem g_w_"), 0);
            overlay.press(Press::Tab, 0);
            let asking = overlay.asking;
            overlay.complete(&facts);
            let offered = overlay.candidates.clone();
            overlay.press(Press::Tab, 0);
            let ran = overlay.press(Press::Enter, 0).lines;
            let picked = overlay.edit.as_str();
            check(
                asking
                    && alone == "givecredits "
                    && offered == ["g_w_blstrpstl001", "g_w_lghtsbr01"]
                    && ran.is_empty()
                    && picked == "giveitem g_w_lghtsbr01 "
                    && overlay.candidates.is_empty(),
                format!(
                    "one choice {alone:?}, offered {offered:?}, Enter ran {ran:?} and left {picked:?}"
                ),
            )
        }),
        ("Escape closes the choices first, then the console", {
            let mut overlay = Overlay {
                open: true,
                candidates: vec!["a".into(), "b".into()],
                chosen: Some(0),
                ..Overlay::default()
            };
            overlay.press(Press::Escape, 0);
            let first = (overlay.open, overlay.candidates.len());
            overlay.press(Press::Escape, 0);
            check(
                first == (true, 0) && !overlay.open,
                format!(
                    "after one Escape {first:?}, after two open {}",
                    overlay.open
                ),
            )
        }),
        ("Page Up scrolls the log no further than its length", {
            let mut overlay = Overlay::default();
            overlay.press(Press::PageUp, 5);
            let up = overlay.scrolled;
            overlay.press(Press::PageDown, 5);
            check(
                up == 5 && overlay.scrolled == 0,
                format!("up {up}, down {}", overlay.scrolled),
            )
        }),
        ("a report keeps the mark and the log", {
            let mut log = ConsoleLog::default();
            log.push("> god".into(), false);
            log.push("God mode switched.".into(), false);
            log.echo(&["Cheats are off in this game.".into(), "Saved.".into()]);
            let text = super::report_text(Some(true), &log);
            check(
                text == "cheats used: yes\n> god\nGod mode switched.\nCheats are off in this game.\n",
                format!("{text:?}"),
            )
        }),
    ]
}
