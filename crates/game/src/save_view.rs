//! Saving and loading in the game (`docs/UI.md`), the
//! window's side: the saves' folder listed for the screens, what a save
//! made now would be called, the screens' Save and Delete, quick save and
//! quick load (F5, F9), and what became of a save. The host writes it
//! (`sim_host::save_world`); a load goes through the session flow (from the
//! menu, or the program again into it during play, only played alone),
//! except the reload after the party fell, which the host makes in place
//! for every player from its own latest save (`sim_host::ReloadRequest`,
//! I30 P13). A copy of another host's save is listed under its own name,
//! marked with whose it is (`SaveView::from`).

use std::path::PathBuf;

use bevy::prelude::*;
use ui::intent::{Intent, SessionIntent};
use ui::vm::SaveView;

use crate::settings::{Action, KeyMap};
use crate::sim_host::{ReloadRequest, SceneSwap, WorldHere};
use crate::ui_view::{Ui, UiIntents};

/// A save asked for: over `slot` (a save's id) or a new one, named `name`,
/// in the area called `area`; by the Save screen (`on_screen`: a refusal
/// is told in a message) or a key or the console (only in the feedback
/// log: a message would hold the player in the middle of a fight).
pub struct Asked {
    pub slot: Option<String>,
    pub name: String,
    pub area: String,
    pub on_screen: bool,
}

/// The save the host makes next (`sim_host::save_world`).
#[derive(Resource, Default)]
pub struct SaveRequest(pub Option<Asked>);

/// What became of a save: its id, or why it was refused.
pub struct Saved {
    pub result: Result<String, String>,
    pub on_screen: bool,
}

#[derive(Resource, Default)]
pub struct SaveOutcome(pub Option<Saved>);

/// The host's save copies this player received (I30 P17): each one's id
/// here, or why it was refused (`net_join`).
#[derive(Resource, Default)]
pub struct Received(pub Vec<Result<String, String>>);

/// The quick save's name.
pub(crate) const QUICK_NAME: &str = "Quick save";

/// Where the saves are: `KJK_SAVES`, else `saves` beside the settings.
pub fn folder() -> Option<PathBuf> {
    std::env::var_os("KJK_SAVES")
        .map(PathBuf::from)
        .or_else(|| crate::settings::folder().map(|folder| folder.join("saves")))
}

/// The save `id` in the saves' folder: its world and its meta.
pub fn read(id: &str) -> Result<(sim::Save, saves::Meta), String> {
    let root = folder().ok_or("no folder for saves here")?;
    saves::read(&root, id)
}

/// A refusal told in a message over the screen open (and in the feedback
/// log).
pub(crate) fn refuse_on_screen(ui: &mut Ui, why: String) {
    ui.vm.feedback.push(why.clone());
    ui.vm.session.error = Some(why);
    ui.stack.open("message", &ui.vm);
}

/// When `saved` (Unix seconds) was, as the screens show it (UTC).
fn when(saved: u64) -> String {
    let (days, seconds) = (saved / 86_400, saved % 86_400);
    let (year, month, day) = crate::runtime::civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds / 3600,
        seconds / 60 % 60
    )
}

/// The screens that list the saves.
const LISTING: [&str; 4] = ["solo", "host", "save", "load"];

/// The saves on disk, into the screens' list; one open starts over (its
/// rows moved under it).
fn refresh(ui: &mut Ui) {
    let listing = folder().map_or_else(saves::Listing::default, |root| saves::list(&root));
    for why in &listing.unread {
        warn!(target: "saves", "{why}");
    }
    for screen in LISTING {
        ui.stack.rewind(screen);
    }
    if let Some(assets) = ui.assets.as_mut() {
        ui.vm.new_game = new_game(assets);
    }
    let mut received = listing.received;
    ui.vm.saves = listing
        .saves
        .into_iter()
        .map(|(id, meta)| SaveView {
            name: meta.name,
            from: received.remove(&id).map(|copy| copy.from),
            id,
            area: meta.area,
            module: meta.module,
            when: when(meta.saved),
        })
        .collect();
}

/// A new game's row: the campaign's start module
/// (`ui::intent::new_game_module`), named as its entry area's ARE `Name`
/// (the area a save there would name; the IFO's `Mod_Name` is a code: K1
/// `end_m01aa`, TSL `001EBO`).
fn new_game(assets: &mut ui::assets::GuiAssets) -> SaveView {
    let module = ui::intent::new_game_module(assets.game);
    let area = assets
        .module_resolver(module)
        .and_then(|resolver| {
            let ir = asset_odyssey::module::ModuleIr::build(
                module,
                &crate::viewer::ResolverSource(&resolver),
            )
            .ok()?;
            ir.areas
                .iter()
                .find(|area| area.resref == ir.entry_area)?
                .name_strref
        })
        .and_then(|strref| ui::draw::Textures::text(assets, ui::skin::Source::Campaign, strref))
        .map(|words| fmt_odyssey::code_page(&words))
        .unwrap_or_else(String::new);
    SaveView {
        area,
        module: module.to_string(),
        ..SaveView::default()
    }
}

/// The screens' saves taken: a load they asked for is in `UiIntents` for the
/// session flow.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Acted;

pub struct SaveViewPlugin;

impl Plugin for SaveViewPlugin {
    fn build(&self, app: &mut App) {
        // `SaveRequest` and `SaveOutcome` are the host's (`SimHostPlugin`).
        app.init_resource::<Received>()
            .add_systems(Startup, |mut ui: ResMut<Ui>| refresh(&mut ui))
            .add_systems(
                Update,
                (here, act, told)
                    .chain()
                    .in_set(Acted)
                    .after(crate::ui_view::Stepped),
            );
    }
}

/// What a save made now would be: the area's name, its module; none where
/// the world is another's, or not a module's.
fn here(mut ui: ResMut<Ui>, world: Option<Res<WorldHere>>, swap: Option<Res<SceneSwap>>) {
    let vm = &mut ui.vm;
    vm.load_in_play = world.as_ref().is_some_and(|world| world.alone);
    let module = swap.filter(|_| world.is_some());
    let module = module.as_ref().and_then(|swap| swap.level.module());
    if vm.here.as_ref().map(|here| &here.module) == module.map(|module| &module.name) {
        return;
    }
    vm.here = module.map(|module| SaveView {
        name: module.area_title.clone(),
        area: module.area_title.clone(),
        module: module.name.clone(),
        ..SaveView::default()
    });
}

/// The screens' Save and Delete; the lost box's Load; quick save and quick
/// load, in play with no screen open.
fn act(
    mut ui: ResMut<Ui>,
    mut intents: ResMut<UiIntents>,
    mut request: ResMut<SaveRequest>,
    mut reload: ResMut<ReloadRequest>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<KeyMap>>,
    world: Option<Res<WorldHere>>,
) {
    let ui = &mut *ui;
    // A save with no name is called after its area.
    let ask = |slot: Option<String>, name: String, on_screen: bool, ui: &mut Ui| {
        let Some(here) = ui.vm.here.as_ref() else {
            ui.vm.feedback.push("There is nothing to save here.".into());
            return None;
        };
        Some(Asked {
            slot,
            name: match name.trim() {
                "" => here.name.clone(),
                name => name.to_string(),
            },
            area: here.area.clone(),
            on_screen,
        })
    };
    let mut rest = Vec::new();
    for intent in std::mem::take(&mut intents.0) {
        match intent {
            // The Save screen's (open until the save is made), or the
            // console's.
            Intent::Save { slot, name } => {
                let on_screen = ui.stack.has("save");
                if let Some(asked) = ask(slot, name, on_screen, ui) {
                    request.0 = Some(asked);
                }
            }
            Intent::DeleteSave(id) => {
                let deleted = folder()
                    .ok_or_else(|| "no folder for saves here".to_string())
                    .and_then(|root| saves::delete(&root, &id));
                if let Err(why) = deleted {
                    warn!(target: "saves", "not deleted: {why}");
                }
                refresh(ui);
            }
            // The party fell (I30 P13): the host loads its latest save in
            // place, for everyone (its own: never a copy of another's
            // game); with none, back to the menu. A guest waits for the
            // host's.
            Intent::ReloadLatest => {
                let latest = ui.vm.saves.iter().find(|save| save.from.is_none());
                match (&world, latest) {
                    (Some(_), Some(latest)) => reload.0 = Some(latest.id.clone()),
                    (Some(_), None) => rest.push(Intent::Session(SessionIntent::ToMenu)),
                    (None, _) => ui.vm.feedback.push("The host loads the last save.".into()),
                }
            }
            other => rest.push(other),
        }
    }
    let (Some(keys), Some(_)) = (keys, world) else {
        intents.0 = rest;
        return;
    };
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    if !ui.stack.is_open() {
        if map.pressed_now(&keys, Action::QuickSave)
            && let Some(asked) = ask(Some(saves::QUICK.into()), QUICK_NAME.into(), false, ui)
        {
            request.0 = Some(asked);
        }
        // Loaded as the screens' Load is: the session flow refuses it while
        // hosting, and says why.
        if map.pressed_now(&keys, Action::QuickLoad) {
            if ui.vm.saves.iter().any(|save| save.id == saves::QUICK) {
                rest.push(Intent::Session(SessionIntent::Solo {
                    save: Some(saves::QUICK.into()),
                    character: None,
                    campaign: None,
                }));
            } else {
                ui.vm.feedback.push("There is no quick save yet.".into());
            }
        }
    }
    intents.0 = rest;
}

/// What a save made tells at the window's right edge (`progress::Told`).
const SAVED: &str = "Game saved";

/// A save made: listed, told ("Game saved" at the right edge, under the
/// minimap), and the Save screen that asked closed back to the game;
/// refused, told why (in a message over the Save screen when it asked).
fn told(
    mut ui: ResMut<Ui>,
    mut outcome: ResMut<SaveOutcome>,
    mut received: ResMut<Received>,
    summary: Option<ResMut<crate::progress::Told>>,
) {
    let ui = &mut *ui;
    for kept in received.0.drain(..) {
        match kept {
            Ok(_) => {
                refresh(ui);
                ui.vm
                    .feedback
                    .push("The host's save was copied here.".into());
            }
            Err(why) => ui.vm.feedback.push(why),
        }
    }
    let Some(Saved { result, on_screen }) = outcome.0.take() else {
        return;
    };
    match result {
        Ok(id) => {
            refresh(ui);
            if on_screen {
                ui.stack.clear();
            }
            let quick = id == saves::QUICK;
            ui.vm
                .feedback
                .push(if quick { "Quick saved." } else { "Game saved." }.into());
            if let Some(mut summary) = summary {
                summary.notices.push(SAVED.into());
            }
        }
        Err(why) if on_screen => refuse_on_screen(ui, why),
        Err(why) => ui.vm.feedback.push(why),
    }
}
