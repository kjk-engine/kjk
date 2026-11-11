//! The game's screens, each a KOTOR layout and what its controls do, and
//! the factory that opens them by name.

pub mod arena_panel;
pub mod bark;
pub mod campaign;
pub mod chargen;
pub mod computer;
pub mod container;
pub mod credits;
pub mod dialog;
pub mod feed;
pub mod filter;
pub mod floaty;
pub mod galaxy_map;
pub mod hud;
pub mod join;
pub mod journal;
pub mod key_mapping;
pub mod loading;
pub mod lost;
pub mod main_menu;
pub mod map;
pub mod message;
pub mod new_or_load;
pub mod options;
pub mod options_in_game;
pub mod pages;
pub mod party;
pub mod pazaak;
pub mod power_pick;
pub mod report;
pub mod resolution;
pub mod save_load;
pub mod status_summary;
pub mod store;
pub mod tab;
pub mod tabs;
pub mod transition;
pub mod upgrade;
pub mod workbench;

use crate::intent::Intent;
use crate::screen::{Screen, ScreenId};
use crate::vm::ViewModel;

/// The screen named `name`, if there is one.
pub fn open(name: &str, vm: &ViewModel) -> Option<Box<dyn Screen>> {
    if let Some(page) = options::page(name, vm) {
        return Some(page);
    }
    if let Some(then) = name.strip_prefix("campaign:") {
        return Some(Box::new(campaign::Campaign::new(then)));
    }
    if let Some(then) = name.strip_prefix("chargen:") {
        return Some(Box::new(chargen::Chargen::new(then)?));
    }
    if let Some(id) = name.strip_prefix("story_restore:") {
        return Some(Box::new(message::Restore::new(id, vm)));
    }
    if let Some(strref) = name.strip_prefix("message_box:") {
        return Some(Box::new(message::Message::told(strref)?));
    }
    if let Some(at) = (name == "pages")
        .then_some("0")
        .or(name.strip_prefix("pages:"))
    {
        return Some(Box::new(message::Pages::new(at, vm)?));
    }
    if let Some(item) = name.strip_prefix("upgrade:") {
        return Some(Box::new(upgrade::Upgrade::new(item)?));
    }
    if let Some(deal) = name.strip_prefix("store_confirm:") {
        return Some(Box::new(store::Confirm::new(deal)?));
    }
    if let Some(query) = name.strip_prefix("return_confirm:") {
        return Some(Box::new(map::ReturnConfirm::new(query)?));
    }
    if let Some(to) = name.strip_prefix("quit_confirm:") {
        return Some(Box::new(options_in_game::Quit::new(to)?));
    }
    if let Some(asked) = name.strip_prefix("save_overwrite:") {
        return Some(Box::new(save_load::Overwrite::new(asked)));
    }
    if let Some(asked) = name.strip_prefix("save_name:") {
        return Some(Box::new(save_load::SaveName::new(asked)));
    }
    if let Some(item) = name.strip_prefix("inventory:") {
        let page = pages::Inventory::at_datapad(item.parse().ok()?);
        return Some(Box::new(tab::Tab::new(page)));
    }
    if let Some(action) = name.strip_prefix("key_entry:") {
        return Some(Box::new(key_mapping::KeyEntry::new(action, vm)));
    }
    Some(match name {
        "options_keymap" => Box::new(key_mapping::KeyMapping::new(vm)),
        "options_resolution" => Box::new(resolution::Resolution::new(vm)),
        "main_menu" => Box::new(main_menu::MainMenu),
        "join" => Box::new(join::Join::new()),
        "container" => Box::new(container::Container::default()),
        "computer" => Box::new(computer::Computer),
        "options_in_game" => Box::new(tab::Tab::new(options_in_game::OptionsInGame)),
        "equip" => Box::new(tab::Tab::new(pages::Equip::default())),
        "inventory" => Box::new(tab::Tab::new(pages::Inventory::default())),
        "character" => Box::new(tab::Tab::new(pages::Character)),
        "abilities" => Box::new(tab::Tab::new(pages::Abilities::default())),
        "messages" => Box::new(tab::Tab::new(pages::Messages::default())),
        "map" => Box::new(tab::Tab::new(map::Map::default())),
        "journal" => Box::new(tab::Tab::new(journal::Journal::default())),
        "store" => Box::new(store::Store::default()),
        "upgrade_select" => Box::new(upgrade::UpgradeSelect::default()),
        "workbench" => Box::new(workbench::Workbench::new(false)),
        "chemical" => Box::new(workbench::Workbench::new(true)),
        "levelup" => Box::new(chargen::Chargen::level_up(vm)?),
        "death" => Box::new(message::Message::new(crate::strings::FALLEN)),
        "lost" => Box::new(lost::Lost::new(vm)),
        "party" => Box::new(party::Party::default()),
        "galaxymap" => Box::new(galaxy_map::GalaxyMap::new(vm)?),
        "credits" => Box::new(credits::Credits::default()),
        "pazaak_wager" => Box::new(pazaak::Wager::default()),
        "pazaak_setup" => Box::new(pazaak::Setup::default()),
        "pazaak_game" => Box::new(pazaak::Game::default()),
        "solo" => Box::new(new_or_load::NewOrLoad::solo()),
        "save" => Box::new(save_load::SaveLoad::saving()),
        "load" => Box::new(save_load::SaveLoad::loading()),
        "host" => Box::new(new_or_load::NewOrLoad::host()),
        "loading" => Box::new(loading::Loading),
        "report" => Box::new(report::Report::default()),
        "message" => Box::new(message::Message::new(
            vm.session.error.as_deref().unwrap_or(""),
        )),
        _ => return None,
    })
}

/// Any layout, shown as it is: nothing it holds does anything, Back
/// closes it (`ui show <layout>`).
pub struct Layout(pub String);

impl Screen for Layout {
    fn id(&self) -> ScreenId {
        ScreenId("layout")
    }

    fn layout(&self) -> &str {
        &self.0
    }

    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}
}

/// A copy of `gui`'s control `from`, tagged `tag`, one row's pitch below it
/// (the gap above it, from the control over it in its column), its focus
/// threaded between `from` and what `from` led down to. Nothing without
/// `from`.
pub(crate) fn copy_below(gui: &mut fmt_odyssey::gui::Gui, from: &str, tag: &str) {
    let Some(original) = gui.find(from).cloned() else {
        return;
    };
    let above = gui
        .controls
        .iter()
        .filter(|control| control.extent.left == original.extent.left)
        .map(|control| control.extent.top)
        .filter(|top| *top < original.extent.top)
        .max();
    let pitch = above.map_or(original.extent.height, |top| original.extent.top - top);
    let id = gui
        .controls
        .iter()
        .map(|control| control.id)
        .max()
        .unwrap_or(0)
        + 1;
    let mut copy = fmt_odyssey::gui::Control {
        id,
        tag: tag.into(),
        ..original.clone()
    };
    copy.extent.top += pitch;
    if let Some(links) = copy.moveto.as_mut() {
        links.up = original.id;
    }
    for control in &mut gui.controls {
        let Some(links) = control.moveto.as_mut() else {
            continue;
        };
        if control.id == original.id {
            links.down = id;
        } else if links.up == original.id {
            links.up = id;
        }
    }
    let at = gui
        .controls
        .iter()
        .position(|control| control.tag == original.tag);
    gui.controls
        .insert(at.map_or(gui.controls.len(), |at| at + 1), copy);
}

/// Copies of `gui`'s control `style`, tagged `tags`, stacked under the
/// control `after` with `gap` between them and as wide as it, focus
/// threaded from `after` through them to what `after` led down to.
/// Nothing without both.
pub(crate) fn stack_under(
    gui: &mut fmt_odyssey::gui::Gui,
    (style, after): (&str, &str),
    tags: &[&str],
    gap: i32,
) {
    let (Some(style), Some(after)) = (gui.find(style).cloned(), gui.find(after).cloned()) else {
        return;
    };
    let below = after.moveto.as_ref().map(|links| links.down);
    let first = gui
        .controls
        .iter()
        .map(|control| control.id)
        .max()
        .unwrap_or(0)
        + 1;
    let mut top = after.extent.top + after.extent.height + gap;
    let mut up = after.id;
    for (n, tag) in tags.iter().enumerate() {
        let id = first + n as i32;
        let mut copy = fmt_odyssey::gui::Control {
            id,
            tag: (*tag).into(),
            ..style.clone()
        };
        copy.extent.top = top;
        copy.extent.left = after.extent.left;
        copy.extent.width = after.extent.width;
        if let Some(links) = copy.moveto.as_mut() {
            links.up = up;
            links.down = if n + 1 < tags.len() {
                id + 1
            } else {
                below.unwrap_or(id)
            };
        }
        top += style.extent.height + gap;
        up = id;
        gui.controls.push(copy);
    }
    let last = up;
    for control in &mut gui.controls {
        let Some(links) = control.moveto.as_mut() else {
            continue;
        };
        if control.id == after.id {
            links.down = first;
        } else if below == Some(control.id) && links.up == after.id {
            links.up = last;
        }
    }
}

/// `gui`'s control `tag` moved under the control `after`, `gap` below it and
/// as wide as it (its focus links kept). Nothing without both.
pub(crate) fn move_under(gui: &mut fmt_odyssey::gui::Gui, (tag, after): (&str, &str), gap: i32) {
    let Some(after) = gui.find(after).map(|after| after.extent) else {
        return;
    };
    if let Some(control) = gui.controls.iter_mut().find(|control| control.tag == tag) {
        control.extent.top = after.top + after.height + gap;
        control.extent.left = after.left;
        control.extent.width = after.width;
    }
}

/// `gui`'s control `tag` drawn as the control `like` is (its kind, borders
/// and text style), kept where it is, as it is named and linked: a check
/// box made a button. Nothing without both.
pub(crate) fn look_like(gui: &mut fmt_odyssey::gui::Gui, (tag, like): (&str, &str)) {
    let Some(like) = gui.find(like).cloned() else {
        return;
    };
    if let Some(control) = gui.controls.iter_mut().find(|control| control.tag == tag) {
        *control = fmt_odyssey::gui::Control {
            id: control.id,
            tag: std::mem::take(&mut control.tag),
            parent: control.parent,
            extent: control.extent,
            moveto: control.moveto.take(),
            ..like
        };
    }
}
