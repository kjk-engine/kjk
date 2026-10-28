//! The galaxy map (`galaxymap`), which a script shows (`ShowGalaxyMap`) on
//! the planet the party is at: the planets the scripts made available on
//! their buttons (`planetary.2da`'s `guitag`), those not selectable greyed,
//! the rest hidden. A planet pressed is chosen; the chosen one's name,
//! description and turning model show, and it is lit. Accept travels there
//! (`GameAction::Travel`) when it is selectable; Back closes the map. Every
//! player sees the map; only the host's Accept travels (I30 P6), a guest's
//! is hidden.
//!
//! A KOTOR 1 campaign keeps KOTOR 1's own layout: KOTOR 2's holds KOTOR 2's
//! planets (`skin::OWN`).

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{disabled, hidden};
use crate::vm::{GalaxyView, PlanetView, SceneView, ViewModel};

pub struct GalaxyMap {
    /// The planet chosen (`planetary.2da` row).
    chosen: u8,
    /// Where the focus starts: the planet the map opened on, when it may be
    /// chosen.
    focus: Option<String>,
}

const NAME: &str = "LBL_PLANETNAME";
const DESCRIPTION: &str = "LBL_DESC";
const MODEL: &str = "3D_PlanetModel";
const ACCEPT: &str = "BTN_ACCEPT";
const BACK: &str = "BTN_BACK";
/// The layout's planet buttons start so (`LBL_Planet_Taris`, `LBL_Live01`).
const PLANETS: [&str; 2] = ["LBL_Planet_", "LBL_Live"];

fn planet<'v>(galaxy: &'v GalaxyView, tag: &str) -> Option<&'v PlanetView> {
    galaxy
        .planets
        .iter()
        .find(|planet| planet.tag.eq_ignore_ascii_case(tag))
}

impl GalaxyMap {
    pub fn new(vm: &ViewModel) -> Option<GalaxyMap> {
        let galaxy = vm.galaxy.as_ref()?;
        let focus = galaxy
            .planets
            .iter()
            .find(|planet| planet.row == galaxy.current && planet.selectable)
            .map(|planet| planet.tag.clone());
        Some(GalaxyMap {
            chosen: galaxy.current,
            focus,
        })
    }

    fn chosen<'v>(&self, galaxy: &'v GalaxyView) -> Option<&'v PlanetView> {
        galaxy
            .planets
            .iter()
            .find(|planet| planet.row == self.chosen)
    }

    /// The host may travel to the chosen planet.
    fn travels(&self, galaxy: &GalaxyView) -> bool {
        galaxy.host && self.chosen(galaxy).is_some_and(|planet| planet.selectable)
    }
}

impl Screen for GalaxyMap {
    fn id(&self) -> ScreenId {
        ScreenId("galaxymap")
    }

    fn layout(&self) -> &str {
        "galaxymap"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let Some(galaxy) = vm.galaxy.as_ref() else {
            return ControlState::default();
        };
        let chosen = self.chosen(galaxy);
        let said = |words: Option<&Vec<u8>>| ControlState {
            text: Some(words.cloned().unwrap_or(Vec::new())),
            ..ControlState::default()
        };
        match control.tag.as_str() {
            NAME => said(chosen.map(|planet| &planet.name)),
            DESCRIPTION => said(chosen.map(|planet| &planet.description)),
            MODEL => match chosen.and_then(|planet| planet.model.clone()) {
                Some(model) => ControlState {
                    scene: Some(SceneView::Model(model)),
                    text: Some(Vec::new()),
                    ..ControlState::default()
                },
                None => hidden(),
            },
            ACCEPT if !galaxy.host => hidden(),
            ACCEPT if !self.travels(galaxy) => disabled(),
            tag if PLANETS.iter().any(|prefix| tag.starts_with(prefix)) => {
                match planet(galaxy, tag) {
                    Some(planet) => ControlState {
                        selected: planet.row == self.chosen,
                        disabled: !planet.selectable,
                        ..ControlState::default()
                    },
                    None => hidden(),
                }
            }
            "3D_PlanetDisplay" => said(None),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(galaxy) = vm.galaxy.as_ref() else {
            return;
        };
        match tag {
            ACCEPT if self.travels(galaxy) => {
                out.push(Intent::Game(GameAction::Travel(self.chosen)));
                out.push(Intent::Close);
            }
            BACK => out.push(Intent::Close),
            tag => {
                if let Some(planet) = planet(galaxy, tag).filter(|planet| planet.selectable) {
                    self.chosen = planet.row;
                }
            }
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some(self.focus.as_deref().unwrap_or(BACK))
    }
}
