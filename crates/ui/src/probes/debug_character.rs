//! The Debug Mode character page's probes (`screens::debug_character`):
//! each row shows its name and value with KOTOR 2's arrows; a click on a
//! row's minus arrow steps it down and elsewhere up; Left and Right step the
//! row last pointed at; Restore shows only while the view offers it, and
//! Back closes the page.

use super::paint::button;
use super::{Check, check, expect};
use crate::input::Key;
use crate::intent::Intent;
use crate::paint::ROW_MINUS;
use crate::screen::Screen;
use crate::screens::debug_character::DebugCharacter;
use crate::vm::{DebugCharacterView, OptionRowView, ViewModel};

pub(super) fn all() -> Vec<Check> {
    vec![check("debug_character_page", page())]
}

fn page() -> Result<String, String> {
    let row = |name: &[u8], value: &[u8]| OptionRowView {
        name: name.to_vec(),
        value: value.to_vec(),
        help: b"Help".to_vec(),
    };
    let vm = |restore| ViewModel {
        debug_character: Some(DebugCharacterView {
            title: b"Edit Character".to_vec(),
            rows: vec![row(b"Strength", b"16"), row(b"Level", b"20")],
            restore,
        }),
        ..ViewModel::default()
    };
    let shown = vm(true);
    let mut screen = DebugCharacter::default();
    let list = screen
        .state(&button("LB_OPTIONS", [0, 0, 1, 1], b""), &shown)
        .list
        .ok_or("no list")?;
    let rows: Vec<(Vec<u8>, bool)> = list
        .rows
        .iter()
        .map(|row| (row.text.clone(), row.arrows))
        .collect();
    expect(
        &format!("rows with their values and arrows: {rows:?}"),
        rows == [
            (b"Strength: 16".to_vec(), true),
            (b"Level: 20".to_vec(), true),
        ],
    )?;
    let mut out = Vec::new();
    screen.cell("LB_OPTIONS", 1, ROW_MINUS, &shown);
    screen.row("LB_OPTIONS", 1, &shown, &mut out);
    screen.cell("LB_OPTIONS", 0, 0, &shown);
    screen.row("LB_OPTIONS", 0, &shown, &mut out);
    screen.focused("LB_OPTIONS", &shown);
    screen.pointed("LB_OPTIONS", 1, &shown);
    screen.key(Key::Left, &shown, &mut out);
    screen.key(Key::Right, &shown, &mut out);
    screen.press("BTN_DEFAULT", &shown, &mut out);
    screen.press("BTN_BACK", &shown, &mut out);
    let sent = [
        Intent::DebugStep { row: 1, back: true },
        Intent::DebugStep {
            row: 0,
            back: false,
        },
        Intent::DebugStep { row: 1, back: true },
        Intent::DebugStep {
            row: 1,
            back: false,
        },
        Intent::DebugRestore,
        Intent::Close,
    ];
    expect(&format!("what the page sent: {out:?}"), out == sent)?;
    let hidden = |restore| {
        screen
            .state(&button("BTN_DEFAULT", [0, 0, 1, 1], b""), &vm(restore))
            .hidden
    };
    expect(
        "Restore shown for a player's character and hidden for a companion",
        !hidden(true) && hidden(false),
    )?;
    Ok("two rows with arrows; arrows, keys, Restore and Back sent as expected".into())
}
