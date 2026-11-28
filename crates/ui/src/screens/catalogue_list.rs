//! The UI catalogue's list of stories (`kjk gui catalogue` without a story,
//! docs/UI.md). It is KOTOR's save list drawn with the stories: each screen
//! module is a row with its status, and its stories are the rows under it.
//! Open on a story row asks the catalogue to open that story live
//! (`Intent::OpenStory`). Open on a screen's row opens the screen's first
//! story.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::save_load::screenshot;
use crate::screens::tab::{hidden, text};
use crate::strings;
use crate::vm::ViewModel;

/// What a row of the list stands for: a screen, or one of its stories (the
/// screen's place in `ViewModel::catalogue`, then the story's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Screen(usize),
    Story(usize, usize),
}

fn entries(vm: &ViewModel) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (at, screen) in vm.catalogue.iter().enumerate() {
        entries.push(Entry::Screen(at));
        entries.extend((0..screen.stories.len()).map(|story| Entry::Story(at, story)));
    }
    entries
}

pub struct CatalogueList {
    /// The row pointed at.
    pointed: usize,
    /// The row the cursor is still to be put on, when the list opens on a
    /// story.
    to: Option<usize>,
}

impl CatalogueList {
    /// The list with the cursor on the story `at`, written `module/name`, or
    /// on the first row when `at` is empty or names no story.
    pub fn new(at: &str, vm: &ViewModel) -> CatalogueList {
        let found = at.split_once('/').and_then(|(module, name)| {
            entries(vm).iter().position(|entry| match *entry {
                Entry::Story(screen, story) => {
                    let screen = &vm.catalogue[screen];
                    screen.module == module && screen.stories[story].name == name
                }
                Entry::Screen(_) => false,
            })
        });
        CatalogueList {
            pointed: found.unwrap_or(0),
            to: found,
        }
    }

    fn entry(&self, vm: &ViewModel) -> Option<Entry> {
        entries(vm).get(self.pointed).copied()
    }

    /// The screen the pointed row belongs to, and its story, if the row is
    /// one.
    fn at<'v>(
        &self,
        vm: &'v ViewModel,
    ) -> Option<(
        &'v crate::vm::CatalogueScreen,
        Option<&'v crate::vm::CatalogueStory>,
    )> {
        match self.entry(vm)? {
            Entry::Screen(screen) => Some((&vm.catalogue[screen], None)),
            Entry::Story(screen, story) => {
                let screen = &vm.catalogue[screen];
                Some((screen, Some(&screen.stories[story])))
            }
        }
    }

    fn rows(&self, vm: &ViewModel) -> ControlState {
        let mut rows: Vec<Row> = entries(vm)
            .into_iter()
            .map(|entry| match entry {
                Entry::Screen(screen) => {
                    let screen = &vm.catalogue[screen];
                    Row {
                        text: strings::bytes(&screen.module),
                        value: Some(strings::bytes(&screen.status)),
                        ..Row::default()
                    }
                }
                Entry::Story(screen, story) => Row {
                    text: strings::bytes(&format!(
                        "- {}",
                        vm.catalogue[screen].stories[story].name
                    )),
                    ..Row::default()
                },
            })
            .collect();
        if rows.is_empty() {
            rows.push(Row {
                text: strings::bytes("No stories found"),
                dimmed: true,
                ..Row::default()
            });
        }
        ControlState {
            list: Some(ListView {
                selected: Some(self.pointed.min(rows.len() - 1)),
                rows,
                ..ListView::default()
            }),
            ..ControlState::default()
        }
    }
}

fn games(names: &[String]) -> String {
    let named: Vec<&str> = names
        .iter()
        .map(|name| match name.as_str() {
            "k1" => "KOTOR 1",
            "tsl" => "KOTOR 2",
            other => other,
        })
        .collect();
    format!("Drawn on {}", named.join(" and "))
}

impl Screen for CatalogueList {
    fn id(&self) -> ScreenId {
        ScreenId("catalogue_list")
    }

    fn layout(&self) -> &str {
        "saveload"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LBL_PANELNAME" => text("UI catalogue"),
            "LB_GAMES" => self.rows(vm),
            "LBL_PLANETNAME" => text(match self.at(vm) {
                Some((screen, _)) => format!("{}, {}", screen.module, screen.status),
                None => String::new(),
            }),
            "LBL_AREANAME" => text(match self.at(vm) {
                Some((_, Some(story))) => games(&story.games),
                Some((screen, None)) => format!("{} stories", screen.stories.len()),
                None => String::new(),
            }),
            "LBL_SCREENSHOT" => screenshot(None),
            "LBL_PM1" | "LBL_PM2" | "LBL_PM3" | "LBL_PCNAME" | "LBL_TIMEPLAYED"
            | "CB_CLOUDSAVE" | "BTN_FILTER" | "BTN_DELETE" | "BTN_BACK" => hidden(),
            "BTN_SAVELOAD" => text("Open"),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag != "BTN_SAVELOAD" {
            return;
        }
        if let Some((screen, story)) = self.at(vm)
            && let Some(story) = story.or(screen.stories.first())
        {
            out.push(Intent::OpenStory(screen.module.clone(), story.name.clone()));
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        self.pointed = index;
    }

    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.pointed = index;
    }

    fn activates_primary(&self, list: &str) -> bool {
        self.primary(list).is_some()
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_GAMES").then_some("BTN_SAVELOAD")
    }

    fn cursor_to(&mut self, _: &ViewModel) -> Option<(&'static str, usize)> {
        self.to.take().map(|row| ("LB_GAMES", row))
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_GAMES")
    }
}
