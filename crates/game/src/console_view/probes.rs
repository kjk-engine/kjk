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

/// The facts of a level with two items.
fn facts() -> commands::Facts {
    commands::Facts {
        loaded: true,
        items: Some(vec!["g_w_blstrpstl001".into(), "g_w_lghtsbr01".into()]),
        ..commands::Facts::default()
    }
}

/// Presses in turn, each in a frame of its own: after each, the suggestions
/// and the hint are made again as `run_lines` makes them. Every line run.
fn framed(overlay: &mut Overlay, facts: &commands::Facts, presses: Vec<Press>) -> Vec<String> {
    let mut ran = Vec::new();
    for press in presses {
        ran.extend(overlay.press(press, 0).lines);
        if overlay.wants_facts() {
            overlay.suggest(facts);
        }
    }
    ran
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
        (
            "Tab fills in a single match and picks the first of several",
            {
                let facts = facts();
                let mut overlay = Overlay::default();
                framed(&mut overlay, &facts, vec![text("givecredi"), Press::Tab]);
                let alone = overlay.edit.as_str();
                overlay.edit.clear();
                framed(
                    &mut overlay,
                    &facts,
                    vec![text("giveitem g_w_"), Press::Tab, Press::Tab],
                );
                let offered = overlay.candidates.clone();
                let ran = framed(&mut overlay, &facts, vec![Press::Enter]);
                let picked = overlay.edit.as_str();
                check(
                    alone == "givecredits "
                        && offered == ["g_w_blstrpstl001", "g_w_lghtsbr01"]
                        && ran.is_empty()
                        && picked == "giveitem g_w_lghtsbr01 ",
                    format!(
                        "one match {alone:?}, offered {offered:?}, Enter ran {ran:?} and left {picked:?}"
                    ),
                )
            },
        ),
        (
            "typing makes the suggestions again, and an empty line shows none",
            {
                let facts = facts();
                let mut overlay = Overlay::default();
                framed(&mut overlay, &facts, vec![text("give")]);
                let give = overlay.candidates.clone();
                framed(&mut overlay, &facts, vec![text("i")]);
                let givei = overlay.candidates.clone();
                let backs = vec![Press::Backspace; 5];
                framed(&mut overlay, &facts, backs);
                let empty = overlay.candidates.len();
                let starts = |list: &[String], with: &str| {
                    !list.is_empty() && list.iter().all(|word| word.starts_with(with))
                };
                check(
                    starts(&give, "give")
                        && give.len() > givei.len()
                        && starts(&givei, "givei")
                        && empty == 0,
                    format!("after give {give:?}, after givei {givei:?}, empty line {empty}"),
                )
            },
        ),
        ("Enter runs the line as typed while suggestions show", {
            let facts = facts();
            let mut overlay = Overlay::default();
            let ran = framed(&mut overlay, &facts, vec![text("givei"), Press::Enter]);
            check(ran == ["givei"], format!("ran {ran:?}"))
        }),
        (
            "Up and Down pick a suggestion, and Enter takes the one picked",
            {
                let facts = facts();
                let mut overlay = Overlay::default();
                let moves = vec![text("giveitem g_w_"), Press::Down, Press::Down, Press::Up];
                framed(&mut overlay, &facts, moves);
                let chosen = overlay.chosen;
                let ran = framed(&mut overlay, &facts, vec![Press::Enter]);
                let picked = overlay.edit.as_str();
                check(
                    chosen == Some(0) && ran.is_empty() && picked == "giveitem g_w_blstrpstl001 ",
                    format!("picked {chosen:?}, Enter ran {ran:?} and left {picked:?}"),
                )
            },
        ),
        ("Escape closes the suggestions first, then the console", {
            let facts = facts();
            let mut overlay = Overlay {
                open: true,
                ..Overlay::default()
            };
            framed(&mut overlay, &facts, vec![text("give")]);
            let shown = overlay.candidates.len();
            framed(&mut overlay, &facts, vec![Press::Escape]);
            let first = (overlay.open, overlay.candidates.len());
            framed(&mut overlay, &facts, vec![Press::Escape]);
            check(
                shown > 0 && first == (true, 0) && !overlay.open,
                format!(
                    "{shown} shown, after one Escape {first:?}, after two open {}",
                    overlay.open
                ),
            )
        }),
        ("with no suggestions, Up and Down walk the history", {
            let facts = facts();
            let mut overlay = Overlay::default();
            let lines = vec![text("help"), Press::Enter, text("god"), Press::Enter];
            framed(&mut overlay, &facts, lines);
            let mut seen = Vec::new();
            for press in [Press::Up, Press::Up, Press::Down] {
                framed(&mut overlay, &facts, vec![press]);
                seen.push((overlay.edit.as_str(), overlay.candidates.len()));
            }
            let want = [("god", 0), ("help", 0), ("god", 0)].map(|(line, n)| (line.to_string(), n));
            check(seen == want, format!("walked {seen:?}"))
        }),
        ("the hint shows the arguments still to type", {
            let facts = facts();
            let hints = [
                "giveitem",
                "giveitem ",
                "giveitem g_w_lghtsbr01 ",
                "skill ",
                "skill security",
                "help ",
            ]
            .map(|line| {
                let mut overlay = Overlay::default();
                framed(&mut overlay, &facts, vec![text(line)]);
                overlay.hint
            });
            let want = [
                "",
                "TEMPLATE [COUNT]",
                "[COUNT]",
                "NAME RANKS [WHO]",
                " RANKS [WHO]",
                "[NAME]",
            ];
            check(hints == want, format!("hints {hints:?}"))
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
            log.push("God mode: ON.".into(), false);
            log.echo(&["Cheats are disabled in this game.".into(), "Saved.".into()]);
            let text = super::report_text(Some(true), &log);
            check(
                text == "cheats used: yes\n> god\nGod mode: ON.\nCheats are disabled in this game.\n",
                format!("{text:?}"),
            )
        }),
        (
            "Ctrl+L clears log across frames and Shift+Tab cycles backwards",
            {
                let mut world = world(true);
                let send = |world: &mut World,
                            code: KeyCode,
                            logical: Key,
                            text: Option<&str>,
                            state: ButtonState| {
                    if state == ButtonState::Pressed {
                        world.resource_mut::<ButtonInput<KeyCode>>().press(code);
                    } else {
                        world.resource_mut::<ButtonInput<KeyCode>>().release(code);
                    }
                    world
                        .resource_mut::<Messages<KeyboardInput>>()
                        .write(KeyboardInput {
                            key_code: code,
                            logical_key: logical,
                            state,
                            text: text.map(Into::into),
                            repeat: false,
                            window: Entity::PLACEHOLDER,
                        });
                    let _ = world.run_system_once(capture);
                    world.resource_mut::<ButtonInput<KeyCode>>().clear();
                    world.resource_mut::<Messages<KeyboardInput>>().update();
                };
                world
                    .resource_mut::<ConsoleLog>()
                    .push("old line".into(), false);
                send(
                    &mut world,
                    KeyCode::Backquote,
                    Key::Character("`".into()),
                    Some("`"),
                    ButtonState::Pressed,
                );
                send(
                    &mut world,
                    KeyCode::Backquote,
                    Key::Character("`".into()),
                    None,
                    ButtonState::Released,
                );
                send(
                    &mut world,
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                );
                send(
                    &mut world,
                    KeyCode::KeyL,
                    Key::Character("l".into()),
                    Some("l"),
                    ButtonState::Pressed,
                );
                send(
                    &mut world,
                    KeyCode::KeyL,
                    Key::Character("l".into()),
                    None,
                    ButtonState::Released,
                );
                send(
                    &mut world,
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Released,
                );
                let cleared =
                    world.resource::<ConsoleLog>().0.is_empty() && state(&world).1.is_empty();
                // Test Shift+Tab via capture by loading candidates into world overlay:
                world.resource_mut::<Overlay>().candidates =
                    vec!["a".into(), "b".into(), "c".into()];
                world.resource_mut::<Overlay>().refresh = false;
                send(
                    &mut world,
                    KeyCode::ShiftLeft,
                    Key::Shift,
                    None,
                    ButtonState::Pressed,
                );
                send(
                    &mut world,
                    KeyCode::Tab,
                    Key::Tab,
                    None,
                    ButtonState::Pressed,
                );
                send(
                    &mut world,
                    KeyCode::Tab,
                    Key::Tab,
                    None,
                    ButtonState::Released,
                );
                send(
                    &mut world,
                    KeyCode::ShiftLeft,
                    Key::Shift,
                    None,
                    ButtonState::Released,
                );
                let back_tab = world.resource::<Overlay>().chosen;
                check(
                    cleared && back_tab == Some(2),
                    format!("cleared {cleared}, back_tab {back_tab:?}"),
                )
            },
        ),
        (
            "non-ASCII CP1252 editing and single-line trailing newline paste",
            {
                let facts = facts();
                let mut overlay = Overlay::default();
                // 'é' is 1 byte in CP1252 and 2 bytes in UTF-8; pick() slices before caret.
                framed(
                    &mut overlay,
                    &facts,
                    vec![text("find café; givei"), Press::Tab],
                );
                let picked_ok = overlay.edit.as_str() == "find café; giveitem ";
                framed(
                    &mut overlay,
                    &facts,
                    vec![Press::Left, Press::Left, Press::End],
                );
                let single = overlay.press(Press::Paste("g_w_lghtsbr01\n".into()), 0);
                let pasted = overlay.edit.as_str();
                check(
                    picked_ok
                        && single.lines.is_empty()
                        && pasted == "find café; giveitem g_w_lghtsbr01",
                    format!("picked_ok {picked_ok}, single {single:?}, pasted {pasted:?}"),
                )
            },
        ),
    ]
}
