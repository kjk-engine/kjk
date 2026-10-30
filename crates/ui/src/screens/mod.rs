//! The game's screens, each a KOTOR layout and what its controls do, and
//! the factory that opens them by name.

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
    if let Some(strref) = name.strip_prefix("message_box:") {
        return Some(Box::new(message::Message::told(strref)?));
    }
    if let Some(item) = name.strip_prefix("upgrade:") {
        return Some(Box::new(upgrade::Upgrade::new(item)?));
    }
    if let Some(deal) = name.strip_prefix("store_confirm:") {
        return Some(Box::new(store::Confirm::new(deal)?));
    }
    if let Some(asked) = name.strip_prefix("save_overwrite:") {
        return Some(Box::new(save_load::Overwrite::new(asked)));
    }
    if let Some(asked) = name.strip_prefix("save_name:") {
        return Some(Box::new(save_load::SaveName::new(asked)));
    }
    if let Some(action) = name.strip_prefix("key_entry:") {
        return Some(Box::new(key_mapping::KeyEntry::new(action, vm)));
    }
    Some(match name {
        "options_keymap" => Box::new(key_mapping::KeyMapping::new(vm)),
        "options_resolution" => Box::new(resolution::Resolution::new(vm)),
        "main_menu" => Box::new(main_menu::MainMenu),
        "join" => Box::new(join::Join::new()),
        "container" => Box::new(container::Container),
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
        "pages" => Box::new(message::Pages::default()),
        "lost" => Box::new(lost::Lost),
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
