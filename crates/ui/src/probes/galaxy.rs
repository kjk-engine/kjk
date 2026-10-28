//! The galaxy map and the game's end over layouts holding KOTOR's tags, on
//! hand-written view-models.

use fmt_odyssey::gui::Control;

use super::chargen::presses;
use super::menus::{keys, root};
use super::paint::{Fake, button};
use super::vm_screens::{disabled, label, list, opened, words};
use super::{Check, check, expect};
use crate::input::{Key, UiInput};
use crate::intent::{GameAction, Intent, SessionIntent};
use crate::vm::{GalaxyView, PlanetView, SceneView, ViewModel};

const TARIS: u8 = 1;
const DANTOOINE: u8 = 3;

fn layouts(assets: &mut Fake) {
    let mut controls: Vec<Control> = [
        "LBL_Planet_Taris",
        "LBL_Planet_Dantooine",
        "LBL_Planet_Korriban",
        "BTN_ACCEPT",
        "BTN_BACK",
    ]
    .iter()
    .enumerate()
    .map(|(at, tag)| button(tag, [400, 10 + at as i32 * 30, 24, 24], b""))
    .collect();
    controls.extend(
        ["LBL_PLANETNAME", "LBL_DESC", "3D_PlanetModel"]
            .iter()
            .enumerate()
            .map(|(at, tag)| label(tag, 200 + at as i32 * 20)),
    );
    assets.layouts.insert("galaxymap", root(640, 480, controls));
    assets
        .layouts
        .insert("credits", root(640, 480, vec![list("LB_CREDITS", 40)]));
}

fn planet(row: u8, tag: &str, name: &str, selectable: bool) -> PlanetView {
    PlanetView {
        row,
        tag: tag.into(),
        name: name.as_bytes().to_vec(),
        description: format!("{name}'s words").into_bytes(),
        model: Some(format!("planet_{row:02}")),
        selectable,
    }
}

fn galaxy_vm(host: bool) -> ViewModel {
    ViewModel {
        galaxy: Some(GalaxyView {
            planets: vec![
                planet(TARIS, "LBL_Planet_Taris", "Taris", false),
                planet(DANTOOINE, "LBL_Planet_Dantooine", "Dantooine", true),
            ],
            current: DANTOOINE,
            host,
        }),
        ..ViewModel::default()
    }
}

fn travels(intents: &[Intent]) -> Vec<u8> {
    intents
        .iter()
        .filter_map(|intent| match intent {
            Intent::Game(GameAction::Travel(planet)) => Some(*planet),
            _ => None,
        })
        .collect()
}

/// The host's map: Korriban (not available) hidden, Taris greyed and not
/// chosen when pressed, Dantooine (the current planet) chosen with its name,
/// words and model; Accept travels there and closes. A guest's has no
/// Accept.
fn galaxy_map() -> Check {
    let mut assets = Fake::new();
    layouts(&mut assets);
    let vm = galaxy_vm(true);
    let mut map = opened("galaxymap", &mut assets, &vm);
    let shown = (
        map.hidden("LBL_Planet_Korriban", &vm),
        disabled(&map, &vm, "LBL_Planet_Taris"),
        map.state_of("LBL_Planet_Dantooine", &vm)
            .is_some_and(|state| state.selected),
        map.focused().map(str::to_string),
    );
    presses(&mut map, &mut assets, &vm, &["LBL_Planet_Taris"]);
    let chosen = (
        words(&map, &vm, "LBL_PLANETNAME"),
        words(&map, &vm, "LBL_DESC"),
        map.state_of("3D_PlanetModel", &vm)
            .and_then(|state| state.scene),
    );
    let accepted = presses(&mut map, &mut assets, &vm, &["BTN_ACCEPT"]);
    let closed = !map.is_open();
    let guest = galaxy_vm(false);
    let mut theirs = opened("galaxymap", &mut assets, &guest);
    let no_accept = theirs.hidden("BTN_ACCEPT", &guest);
    let back = presses(&mut theirs, &mut assets, &guest, &["BTN_BACK"]);
    check("galaxy_map", {
        expect(
            &format!("Korriban hidden, Taris greyed, Dantooine chosen and focused: {shown:?}"),
            shown == (true, true, true, Some("LBL_Planet_Dantooine".into())),
        )
        .and_then(|()| {
            expect(
                &format!("Dantooine's name, words and model: {chosen:?}"),
                chosen
                    == (
                        "Dantooine".into(),
                        "Dantooine's words".into(),
                        Some(SceneView::Model("planet_03".into())),
                    ),
            )
        })
        .and_then(|()| {
            expect(
                &format!("Accept travels to Dantooine and closes: {accepted:?}"),
                travels(&accepted) == [DANTOOINE] && closed,
            )
        })
        .and_then(|()| {
            expect(
                &format!("a guest's map has no Accept, its Back closes: {back:?}"),
                no_accept && travels(&back).is_empty() && !theirs.is_open(),
            )
        })
        .map(|()| "planets shown, chosen, travelled to by the host".into())
    })
}

/// The credits rise; the main menu once the last has gone, asked once;
/// Back asks it at once.
fn credits() -> Check {
    let mut assets = Fake::new();
    layouts(&mut assets);
    let vm = ViewModel {
        credits: vec![b"- BIOWARE -".to_vec(), b"Producer\nNAME".to_vec()],
        ..ViewModel::default()
    };
    let to_menu = |intents: &[Intent]| {
        intents
            .iter()
            .filter(|intent| **intent == Intent::Session(SessionIntent::ToMenu))
            .count()
    };
    let second = UiInput {
        dt: 1.0,
        ..keys(&[])
    };
    let mut end = opened("credits", &mut assets, &vm);
    let at_first = to_menu(&end.step(&mut assets, &vm, &second).intents);
    let mut later = 0;
    for _ in 0..60 {
        later += to_menu(&end.step(&mut assets, &vm, &second).intents);
    }
    let risen = end
        .state_of("LB_CREDITS", &vm)
        .and_then(|state| state.list)
        .map(|list| (list.roll.is_some(), list.rows.len()));
    let mut skipped = opened("credits", &mut assets, &vm);
    let back = to_menu(&skipped.step(&mut assets, &vm, &keys(&[Key::Back])).intents);
    check(
        "credits",
        expect(
            &format!(
                "rows {risen:?}; the menu asked at first {at_first}, after a minute {later}, on Back {back}"
            ),
            risen == Some((true, 2)) && at_first == 0 && later == 1 && back == 1,
        )
        .map(|()| "the credits roll, then the main menu".into()),
    )
}

pub(super) fn all() -> Vec<Check> {
    vec![galaxy_map(), credits()]
}
