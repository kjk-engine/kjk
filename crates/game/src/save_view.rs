//! Saving and loading in the game (`docs/UI.md`), the
//! window's side: the saves' folder listed for the screens, what a save
//! made now would be called, the screens' Save and Delete, quick save and
//! quick load (F5, F9), and what became of a save. The host writes it
//! (`sim_host::save_world`); a load goes through the session flow: from
//! the menu, or in place during play, only played alone, as the reload
//! after the party fell, which the host makes in place for every player
//! from its own latest save (`sim_host::ReloadRequest`, I30 P13). A copy
//! of another host's save is listed under its own name, marked with whose
//! it is (`SaveView::from`).

use std::path::PathBuf;

use bevy::prelude::*;
use ui::intent::{Intent, SessionIntent};
use ui::vm::SaveView;

use crate::settings::Action;
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

/// An autosave was written (`sim_host::autosave`) since the screens last
/// listed the saves. The list is read again (`told`), so that the lost box's
/// Load and the Load screen see the autosave as the newest save.
#[derive(Resource, Default)]
pub struct Autosaved(pub bool);

/// The host's save copies this player received (I30 P17): each one's id
/// here, or why it was refused (`net_join`).
#[derive(Resource, Default)]
pub struct Received(pub Vec<Result<String, String>>);

/// The quick save's name.
pub(crate) const QUICK_NAME: &str = "Quick save";

/// Where the saves are: `KJK_SAVES`, else `saves` beside the settings.
pub fn folder() -> Option<PathBuf> {
    saves::folder(|name| std::env::var(name).ok())
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

/// The day `saved` (Unix seconds) was, as the lobby shows it (UTC):
/// "2026-11-14".
pub fn date(saved: u64) -> String {
    let (year, month, day) = saves::civil_from_days((saved / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// When `saved` (Unix seconds) was, as the screens show it (UTC).
fn when(saved: u64) -> String {
    let (days, seconds) = (saved / 86_400, saved % 86_400);
    let (year, month, day) = saves::civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds / 3600,
        seconds / 60 % 60
    )
}

/// A save's name as the lists show it: the autosaves as such, a damaged
/// one marked (it does not load: `saves::read` refuses it).
fn shown_name(id: &str, name: String, damaged: bool) -> String {
    let name = match saves::auto_slot(id) {
        Some(0) => format!("Autosave: {name}"),
        Some(_) => format!("Autosave (earlier): {name}"),
        None => name,
    };
    if damaged {
        format!("{name} (damaged)")
    } else {
        name
    }
}

/// The screens that list the saves.
const LISTING: [&str; 4] = ["solo", "host", "save", "load"];

/// The campaign the menu or the game is on now. With no screens' assets,
/// KOTOR 1.
fn campaign(ui: &Ui) -> vfs::Game {
    ui.assets
        .as_ref()
        .map_or(vfs::Game::Kotor1, |assets| assets.game)
}

/// The quick save id of the campaign the game is on now. KOTOR 1 and
/// KOTOR 2 each have their own quick save.
pub(crate) fn quick_id(ui: &Ui) -> &'static str {
    saves::quick(campaign(ui).key())
}

/// The game save `id` in `root` belongs to. A save made by this release
/// names its game in its meta. An older save is placed by its story, else
/// by which installed game has the module it was made in.
fn game_of(root: &std::path::Path, id: &str, meta: &saves::Meta) -> vfs::Game {
    meta.game
        .as_deref()
        .and_then(vfs::Game::from_key)
        .unwrap_or_else(|| crate::menu_figure::game_of(meta, saves::read_story(root, id).as_ref()))
}

/// The saves on disk, read again into `Ui::all_saves`, and the list of the
/// campaign's saves shown (`show`).
pub(crate) fn refresh(ui: &mut Ui) {
    let root = folder();
    let listing = root
        .as_deref()
        .map_or_else(saves::Listing::default, saves::list);
    for why in &listing.unread {
        warn!(target: "saves", "{why}");
    }
    if let Some(assets) = ui.assets.as_mut() {
        ui.vm.new_game = new_game(assets);
    }
    let mut received = listing.received;
    let damaged = listing.damaged;
    ui.all_saves = listing
        .saves
        .into_iter()
        .map(|(id, meta)| {
            let game = root
                .as_deref()
                .map_or(vfs::Game::Kotor1, |root| game_of(root, &id, &meta));
            let view = SaveView {
                name: shown_name(&id, meta.name, damaged.contains(&id)),
                from: received.remove(&id).map(|copy| copy.from),
                // An older KOTOR 2 save in one of KOTOR 1's slots moves to
                // KOTOR 2's own.
                id: root
                    .as_deref()
                    .and_then(|root| saves::move_to_own_slot(root, &id, game.key()))
                    .map_or(id, String::from),
                area: meta.area,
                module: meta.module,
                when: when(meta.saved),
                cheats: meta.cheats,
            };
            (game, view)
        })
        .collect();
    show(ui);
}

/// The screens' list made of the saves of the campaign the menu or the game
/// is on now. A KOTOR 1 save is never listed while KOTOR 2 is on, and the
/// other way round. A list screen that is open starts over (its rows moved
/// under it).
fn show(ui: &mut Ui) {
    let game = campaign(ui);
    for screen in LISTING {
        ui.stack.rewind(screen);
    }
    ui.vm.saves = ui
        .all_saves
        .iter()
        .filter(|(of, _)| *of == game)
        .map(|(_, view)| view.clone())
        .collect();
    ui.saves_for = Some(game);
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
        // `SaveRequest`, `SaveOutcome` and `Autosaved` are the host's
        // (`SimHostPlugin`).
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
/// the world is another's, or not a module's. Also keeps `vm.new_game` in
/// sync with the active campaign when `solo` or `host` is open, cached per
/// game so switching campaigns on the main menu does no synchronous disk I/O.
/// When the campaign changes, the screens' list is made again of the new
/// campaign's saves (`show`), from the saves already read.
fn here(
    mut ui: ResMut<Ui>,
    world: Option<Res<WorldHere>>,
    swap: Option<Res<SceneSwap>>,
    mut cached: Local<[Option<SaveView>; 2]>,
) {
    let ui = &mut *ui;
    if ui.saves_for != Some(campaign(ui)) {
        show(ui);
    }
    if (ui.stack.has("solo") || ui.stack.has("host"))
        && let Some(assets) = ui.assets.as_mut()
    {
        let slot = usize::from(assets.game == vfs::Game::Kotor2);
        let entry = cached[slot].get_or_insert_with(|| new_game(assets));
        if ui.vm.new_game.module != entry.module || ui.vm.new_game.area != entry.area {
            ui.vm.new_game = entry.clone();
        }
    }
    let vm = &mut ui.vm;
    vm.load_in_play = world.is_some();
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
    actions: Option<Res<crate::pad::Actions>>,
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
                    (None, _) => ui.vm.feedback.push("Waiting for the host.".into()),
                }
            }
            other => rest.push(other),
        }
    }
    let (Some(actions), Some(_)) = (actions, world) else {
        intents.0 = rest;
        return;
    };
    if !ui.stack.is_open() {
        let quick = quick_id(ui);
        if actions.pressed(Action::QuickSave)
            && let Some(asked) = ask(Some(quick.into()), QUICK_NAME.into(), false, ui)
        {
            request.0 = Some(asked);
        }
        // Loaded as the screens' Load is: the session flow refuses it while
        // hosting, and says why.
        if actions.pressed(Action::QuickLoad) {
            if ui.vm.saves.iter().any(|save| save.id == quick) {
                rest.push(Intent::Session(SessionIntent::Solo {
                    save: Some(quick.into()),
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
/// An autosave is listed without a word.
fn told(
    mut ui: ResMut<Ui>,
    mut outcome: ResMut<SaveOutcome>,
    mut autosaved: ResMut<Autosaved>,
    mut received: ResMut<Received>,
    summary: Option<ResMut<crate::progress::Told>>,
) {
    let ui = &mut *ui;
    if std::mem::take(&mut autosaved.0) {
        refresh(ui);
    }
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
            let quick = id == saves::QUICK || id == saves::TSL_QUICK;
            ui.vm
                .feedback
                .push(if quick { "Quick saved." } else { "Game saved." }.into());
            if let Some(mut summary) = summary {
                summary.notices.push(SAVED.into());
            }
        }
        Err(why) if on_screen => refuse_on_screen(ui, why),
        // A key's save refused is told where "Game saved" would have been,
        // so a quick load after it is not taken for the save just asked.
        Err(why) => {
            if let Some(mut summary) = summary {
                summary.notices.push(why.clone());
            }
            ui.vm.feedback.push(why);
        }
    }
}
