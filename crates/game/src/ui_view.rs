//! KOTOR's screens in the game: each frame the open screens step with the
//! frame's input (`ui_input`) and draw over everything else: a camera of
//! their own draws them into an image the window's size, and another,
//! on whatever the scene draws into (so headless captures show them too),
//! composites it over the world in gamma space (`ui_composite`).
//!
//! The draw list goes to the GPU as runs of quads that share a texture,
//! in the list's order, each run nearer the camera than the last; a
//! quad's clip is cut on the CPU. Images and colours go as their sRGB
//! bytes, unconverted, so the screens blend as KOTOR's do.
//!
//! Intents that leave the screens wait in `UiIntents` for the systems
//! that act on them; `UiOpen` says whether any screen is open.
//!
//! In play, KOTOR's HUD (`ui::screens::hud`) is drawn first, under the
//! screens, fitted to the window's shape, but not during a conversation;
//! `KotorHud` says it is in use (the fight's own HUD then stands down).
//! Words floating over creatures (`ui::screens::floaty`) go over it, with
//! it; a bark's bubble (`ui::screens::bark`) over them, under the screens;
//! a conversation's screen (`ui::screens::dialog`) in its place while this
//! player talks.

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    ClearColorConfig, OrthographicProjection, Projection, RenderTarget, ScalingMode,
};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::gamepad::Gamepad;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorOptions, PrimaryWindow};
use ui::assets::GuiAssets;
use ui::draw::{DrawList, Quad, TextureId, fit, turn};
use ui::intent::Intent;
use ui::raster::texel;
use ui::screen::Stack;
use ui::vm::ViewModel;

use console::UiRequests;

use crate::lifecycle_view::OverlayCamera;
use crate::ui_composite::{UiComposite, UiCompositePlugin};
use crate::ui_input::{Held, gather};
use crate::ui_material::{UiMaterial, UiMaterialPlugin, UiParams};

/// The screens' render layer: nothing else is on it.
const LAYER: usize = 9;

/// The open screens, what they draw with and what they show.
#[derive(Resource)]
pub struct Ui {
    pub stack: Stack,
    pub assets: Option<GuiAssets>,
    pub vm: ViewModel,
    pub(crate) layers: Vec<DrawList>,
    /// The console's `nav`, `type` and `back`, for this frame's step.
    scripted: Vec<Vec<String>>,
    /// KOTOR's HUD layout, once read (none: it does not read).
    hud: Option<Option<(fmt_odyssey::gui::Gui, ui::skin::Source)>>,
    /// The bark bubble's layout, the same way.
    bark: Option<Option<ui::screens::bark::Bubble>>,
    /// The conversation screen's layout, the same way.
    dialog: Option<Option<ui::screens::dialog::Dialog>>,
    /// The status summary's layout, the same way.
    summary: Option<Option<(fmt_odyssey::gui::Gui, ui::skin::Source)>>,
}

impl Ui {
    /// The conversation screen placed for what it shows now, in a window
    /// of `window` pixels, while there is a conversation and its layout
    /// reads.
    pub fn dialog_placed(&mut self, window: (f32, f32)) -> Option<ui::screens::dialog::Placed> {
        let view = self.vm.dialog.as_ref()?;
        let assets = self.assets.as_mut()?;
        let dialog = self
            .dialog
            .get_or_insert_with(|| ui::screens::dialog::Dialog::read(assets))
            .as_ref()?;
        dialog.place(assets, view, window)
    }
}

/// Whether KOTOR's HUD is drawn this frame.
#[derive(Resource, Default)]
pub struct KotorHud(pub bool);

/// Intents the screens raised for the game (sessions, actions, settings).
#[derive(Resource, Default)]
pub struct UiIntents(pub Vec<Intent>);

/// Whether a screen is open (the character idles, the cursor is free).
#[derive(Resource, Default)]
pub struct UiOpen(pub bool);

/// The camera that composites the screens over the world, the window's
/// size (what the screens are laid out in).
#[derive(Component)]
struct UiCamera;

/// The camera that draws the screens into their image.
#[derive(Component)]
struct ScreensCamera;

/// After the screens stepped: their intents for this frame are in
/// `UiIntents`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stepped;

/// The keyboard's and the mouse's buttons, when there are any.
type Buttons<'w> = (
    Option<Res<'w, ButtonInput<KeyCode>>>,
    Option<Res<'w, ButtonInput<MouseButton>>>,
);

/// A game's state the screens read: its snapshots, the keys.
type InGame<'w> = (
    Option<Res<'w, crate::sim_host::PresentedSnapshot>>,
    Option<Res<'w, crate::settings::KeyMap>>,
);

/// A movie up (it takes the keys and the pointer), the pointer's state (a
/// HUD click only while it is free), and where a portrait or its pazaak
/// icon clicked goes.
type HudPointer<'w, 's> = (
    Option<Res<'w, crate::movie_view::Showing>>,
    Query<'w, 's, &'static CursorOptions, With<PrimaryWindow>>,
    (
        ResMut<'w, crate::leader_view::Picked>,
        ResMut<'w, crate::pazaak_view::WatchAsked>,
        ResMut<'w, crate::weapon_set_view::SwapAsked>,
    ),
);

/// Quads that share a texture, in draw order: positions, texture
/// coordinates and colours, four corners a quad.
struct Run {
    key: (TextureId, bool),
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
}

/// A run's corners, texture coordinates and colours.
type Geometry = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>);

/// A run's entity: its mesh and what it was made of (a run drawn the same
/// as the frame before is not sent again), the material it has, whether
/// it shows.
struct Shown {
    entity: Entity,
    mesh: Handle<Mesh>,
    geometry: Geometry,
    key: (TextureId, bool),
    shown: bool,
}

/// A run's quads as a mesh.
fn run_mesh((positions, uvs, colors): &Geometry) -> Mesh {
    let quads = positions.len() / 4;
    let indices: Vec<u32> = (0..quads as u32)
        .flat_map(|q| [0, 2, 1, 0, 3, 2].map(|i| q * 4 + i))
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone())
    .with_inserted_indices(Indices::U32(indices))
}

/// The GPU side: the camera, each run's entity and mesh, and the images
/// and materials made so far.
#[derive(Resource, Default)]
struct Drawn {
    camera: Option<Entity>,
    /// The screens' camera and image.
    screens: Option<(Entity, Handle<Image>)>,
    runs: Vec<Shown>,
    images: BTreeMap<TextureId, Handle<Image>>,
    materials: BTreeMap<(TextureId, bool), Handle<UiMaterial>>,
    layers: Vec<DrawList>,
    size: Vec2,
    /// The assets' `released` these copies were kept at.
    released: u32,
}

pub struct UiViewPlugin {
    assets: std::sync::Mutex<Option<GuiAssets>>,
    start: Option<&'static str>,
}

impl UiViewPlugin {
    /// Screens drawn from `assets`, opening `start` first.
    pub fn new(assets: Option<GuiAssets>, start: Option<&'static str>) -> UiViewPlugin {
        UiViewPlugin {
            assets: std::sync::Mutex::new(assets),
            start,
        }
    }
}

impl Plugin for UiViewPlugin {
    fn build(&self, app: &mut App) {
        let assets = self.assets.lock().expect("the UI's assets lock").take();
        if assets.is_none() {
            warn!("the game's screens cannot be drawn (see above)");
        }
        let mut stack = Stack::new(ui::screens::open);
        let mut vm = ViewModel::default();
        vm.session.version = format!(
            "kjk {} ({})",
            env!("KJK_VERSION"),
            &env!("KJK_GIT_SHA")[..8.min(env!("KJK_GIT_SHA").len())]
        );
        if let Some(start) = self.start
            && !stack.open(start, &vm)
        {
            warn!("no screen {start}");
        }
        app.add_plugins((
            UiMaterialPlugin,
            UiCompositePlugin,
            crate::gui_scene::GuiScenePlugin,
        ))
        .insert_resource(Ui {
            stack,
            assets,
            vm,
            layers: Vec::new(),
            scripted: Vec::new(),
            hud: None,
            bark: None,
            dialog: None,
            summary: None,
        })
        .init_resource::<UiIntents>()
        .init_resource::<crate::leader_view::Picked>()
        .init_resource::<crate::pazaak_view::WatchAsked>()
        .init_resource::<crate::weapon_set_view::SwapAsked>()
        .init_resource::<KotorHud>()
        .init_resource::<UiOpen>()
        .init_resource::<Drawn>()
        .add_systems(
            Update,
            (requests, step, draw)
                .chain()
                .in_set(Stepped)
                .after(console::ConsoleQueue),
        )
        .add_systems(Last, unclaimed);
    }
}

/// Intents nothing took this frame are dropped (a screen's action no
/// system handles yet), with a note.
fn unclaimed(mut intents: ResMut<UiIntents>) {
    for intent in intents.0.drain(..) {
        debug!(target: "ui", ?intent, "nothing took it");
    }
}

/// The console's `ui` verbs: `open <screen>`, `show <layout>`, `press
/// <TAG>`, `double <LIST> <row>` (a double click on the row, from 0), `nav up|down|left|right`, `type <text>`, `back`, `close`,
/// `security [spike]` (`lock_view`), `use TAG open|close|talk|use`,
/// `wear TEMPLATE`, `unwear TEMPLATE` (`use_view`), `act [N]` (the use key on the target, its Nth action from
/// 0 chosen first, `target_view`), `break`,
/// `resume`, `ready`, `force` (`party_calls`), `holster`, `swap` (the
/// weapon sets, `weapon_set_view`), `form ROW` (TSL's Form of that
/// `spells.2da` row, `form_view`), `portrait 2|3` (a click on
/// the HUD's second or third portrait, `leader_view`), `watch 2|3` (a
/// click on that portrait's pazaak icon, `pazaak_view`), `reply N` (the Nth
/// reply offered, from 1), `leave` (a conversation's, or watching a match's,
/// `pazaak_view`), `accept` and `end` (the host's answer
/// to the first guest asking it in) (`dialogue_view`); and as the screens
/// and keys do, `save [name]`, `quicksave`, `load <id>` (`save_view`).
#[allow(clippy::type_complexity)]
fn requests(
    mut ui: ResMut<Ui>,
    mut pending: ResMut<UiRequests>,
    mut intents: ResMut<UiIntents>,
    (mut shown, mut security, mut uses, mut dressing, mut acted, mut calls, mut picked, mut pinned): (
        ResMut<crate::script_screens::Shown>,
        ResMut<crate::lock_view::Asked>,
        ResMut<crate::use_view::Asked>,
        ResMut<crate::use_view::Dressing>,
        ResMut<crate::target_view::Acted>,
        ResMut<crate::party_calls::Asked>,
        ResMut<crate::leader_view::Picked>,
        ResMut<crate::target_view::Pinned>,
    ),
    (mut replies, mut called, mut watch): (
        Option<ResMut<crate::dialogue_view::ReplyInput>>,
        Option<ResMut<crate::dialogue_view::Called>>,
        ResMut<crate::pazaak_view::WatchAsked>,
    ),
) {
    use sim::script::dialogue::Pick;
    let ui = &mut *ui;
    let mut answer = |pick: Pick| {
        if let Some(replies) = replies.as_deref_mut() {
            replies.pick = Some(pick);
        }
    };
    for request in std::mem::take(&mut pending.0) {
        let words: Vec<&str> = request.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["open", name] => {
                if !ui.stack.open(name, &ui.vm) {
                    warn!(target: "console", "ui: no screen {name}");
                }
            }
            ["show", layout] => ui
                .stack
                .push(Box::new(ui::screens::Layout(layout.to_string()))),
            ["press", tag] => ui.stack.press(tag),
            ["double", tag, row] => match row.parse() {
                Ok(row) => ui.stack.double(tag, row),
                Err(_) => warn!(target: "console", "ui double: {row} is not a row's number"),
            },
            ["close"] => ui.stack.pop(),
            ["script", what @ ..] => match crate::script_screens::parse(what) {
                Some(screen) => shown.0.push(screen),
                None => warn!(target: "console", "ui script: {what:?} is not a script's screen"),
            },
            ["nav" | "type" | "back", ..] => ui.scripted.push(request),
            ["security"] => security.0.push(None),
            ["security", spike] => security.0.push(Some(spike.to_string())),
            ["act"] => acted.0 = true,
            ["target"] => pinned.0 = None,
            ["target", tag] => pinned.0 = Some(tag.to_string()),
            ["act", chosen] => match chosen.parse() {
                Ok(chosen) => *acted = crate::target_view::Acted(true, Some(chosen)),
                Err(_) => warn!(target: "console", "ui act: {chosen} is not an action's number"),
            },
            ["use", tag, word] => match crate::use_view::verb(word) {
                Some(verb) => uses.0.push((tag.to_string(), verb)),
                None => warn!(target: "console", "ui use: {word} is not open, close, talk or use"),
            },
            [verb @ ("wear" | "unwear"), template, on @ ..] if on.len() <= 1 => dressing.0.push((
                template.to_ascii_lowercase(),
                *verb == "wear",
                on.first().map(|tag| tag.to_string()),
            )),
            ["save", name @ ..] => intents.0.push(Intent::Save {
                slot: None,
                name: name.join(" "),
            }),
            ["reload"] => intents.0.push(Intent::ReloadLatest),
            ["quicksave"] => intents.0.push(Intent::Save {
                slot: Some(saves::QUICK.into()),
                name: crate::save_view::QUICK_NAME.into(),
            }),
            ["load", id] => intents
                .0
                .push(Intent::Session(ui::intent::SessionIntent::Solo {
                    save: Some(id.to_string()),
                    character: None,
                    campaign: None,
                })),
            ["break"] => calls.0.push(sim::action::Action::Break),
            ["resume"] => calls.0.push(sim::action::Action::Resume),
            ["ready"] => calls.0.push(sim::action::Action::Ready),
            ["force"] => calls.0.push(sim::action::Action::Force),
            ["holster"] => calls.0.push(sim::action::Action::Holster),
            ["swap"] => calls.0.push(sim::action::Action::SwitchWeapons),
            ["form", row] => match row.parse() {
                Ok(row) => calls.0.push(sim::action::Action::Form(row)),
                Err(_) => warn!(target: "console", "ui form: {row} is not a spells.2da row"),
            },
            ["portrait", n @ ("2" | "3")] => picked.0 = member(&ui.vm, n),
            ["reply", n] => match n.parse::<u8>() {
                Ok(n @ 1..=30) => answer(Pick::Reply(n - 1)),
                _ => warn!(target: "console", "ui reply: {n} is not a reply's number"),
            },
            ["watch", n @ ("2" | "3")] => watch.icon = member(&ui.vm, n),
            ["leave"] => {
                answer(Pick::Leave);
                watch.leave = true;
            }
            [word @ ("accept" | "end")] => {
                if let Some(called) = called.as_deref_mut() {
                    called.0 = Some(*word == "accept");
                }
            }
            other => warn!(target: "console", "ui: {other:?} is not a ui request"),
        }
    }
}

/// The body of the HUD's `n`th portrait (`MemberView::id`), as the
/// console's `ui portrait` and `ui watch` name it.
fn member(vm: &ui::vm::ViewModel, n: &str) -> Option<u32> {
    let place = n.parse::<usize>().ok()?.checked_sub(1)?;
    Some(vm.party.get(place)?.id)
}

/// The console's `ui nav`, `ui type` and `ui back`, as this frame's input.
fn console_input(pending: &[Vec<String>]) -> (Vec<ui::input::Key>, String) {
    use ui::input::Key;
    let (mut keys, mut text) = (Vec::new(), String::new());
    for words in pending {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["nav", "up"] => keys.push(Key::Up),
            ["nav", "down"] => keys.push(Key::Down),
            ["nav", "left"] => keys.push(Key::Left),
            ["nav", "right"] => keys.push(Key::Right),
            ["nav", "activate"] => keys.push(Key::Activate),
            ["back"] => keys.push(Key::Back),
            ["type", rest @ ..] => text.push_str(&rest.join(" ")),
            _ => {}
        }
    }
    (keys, text)
}

#[allow(clippy::too_many_arguments)]
fn step(
    mut ui: ResMut<Ui>,
    (keys, mouse): Buttons,
    (mut typed, mut wheel): (MessageReader<KeyboardInput>, MessageReader<MouseWheel>),
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Camera, With<UiCamera>>,
    time: Res<Time>,
    (mut held, mut last): (Local<Held>, Local<Option<Vec2>>),
    (mut intents, mut open, mut states, mut kotor_hud): (
        ResMut<UiIntents>,
        ResMut<UiOpen>,
        ResMut<console::States>,
        ResMut<KotorHud>,
    ),
    (mut exit, mut facts, replies): (
        MessageWriter<AppExit>,
        ResMut<console::DumpFacts>,
        Option<Res<crate::dialogue_view::ReplyInput>>,
    ),
    heard: Option<ResMut<crate::ui_sounds::Heard>>,
    (presented, map): InGame,
    (showing, cursors, (mut picked, mut watch, mut swap)): HudPointer,
) {
    let top = ui.stack.top().map(|screen| screen.0);
    crate::crash::note(|crumbs| {
        if crumbs.screen.as_deref() != top {
            crumbs.screen = top.map(str::to_owned);
        }
    });
    // A movie over the screens takes the keys and the pointer.
    let movie = showing.is_some_and(|showing| showing.0);
    let (keys, mouse) = if movie { (None, None) } else { (keys, mouse) };
    let window = windows.iter().next();
    let size = cameras
        .iter()
        .next()
        .and_then(Camera::logical_viewport_size)
        .or_else(|| window.map(|window| Vec2::new(window.width(), window.height())))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let mut input = gather(
        keys.as_deref(),
        mouse.as_deref(),
        &mut typed,
        &mut wheel,
        &gamepads,
        window,
        size,
        time.delta_secs(),
        &mut held,
        &mut last,
    );
    let ui = &mut *ui;
    let (more, text) = console_input(&std::mem::take(&mut ui.scripted));
    input.keys.extend(more);
    input.text.push_str(&text);
    if presented.is_some() {
        let map = map.map_or_else(crate::settings::KeyMap::default, |map| map.clone());
        in_game(ui, &mut input, keys.as_deref(), &map);
    }
    ui.stack.veil_world = presented.is_some();
    let frame = match &mut ui.assets {
        Some(assets) => ui.stack.step(assets, &ui.vm, &input),
        None => Default::default(),
    };
    for why in &frame.refusals {
        debug!(target: "ui", "refused: {why}");
    }
    if let Some(mut heard) = heard {
        heard.0.extend(frame.sounds.iter().copied());
    }
    for intent in frame.intents {
        match intent {
            Intent::Quit => {
                exit.write(AppExit::Success);
            }
            Intent::View(member) => ui.vm.viewed = member,
            Intent::Campaign(campaign) => crate::runtime::set_campaign(ui, campaign),
            other => intents.0.push(other),
        }
    }
    facts.0.insert(
        "ui".into(),
        serde_json::json!({
            "screens": ui.stack.screens().map(|screen| screen.0).collect::<Vec<_>>(),
            "focused": ui.stack.focused(),
            "quads": frame.layers.iter().map(|layer| layer.quads.len()).collect::<Vec<_>>(),
            "refusals": frame.refusals,
            "window": [size.x, size.y],
        }),
    );
    let mut layers = frame.layers;
    let hud = presented
        .is_some()
        .then(|| hud_layer(ui, (size.x, size.y), false))
        .flatten();
    kotor_hud.0 = hud.is_some();
    // As KOTOR's, it gives way to a conversation's screen, to a minigame's
    // own (its models' HUD, `minigame_view`), and to a fade (the black
    // between a cutscene's conversations).
    let talking = replies.is_some_and(|replies| replies.active);
    let (minigame, faded) = presented.as_ref().map_or((false, false), |presented| {
        (
            presented.0.current().minigame.is_some(),
            crate::fade_view::covered(&presented.0) > 0.0,
        )
    });
    let mut under = 0;
    if let Some(hud) = hud.filter(|_| !talking && !minigame && !faded) {
        layers.insert(0, hud);
        under = 1;
        for layer in [
            floaty_layer(ui, (size.x, size.y)),
            power_pick_layer(ui, (size.x, size.y)),
            arena_layer(ui, (size.x, size.y)),
        ]
        .into_iter()
        .flatten()
        {
            layers.insert(under, layer);
            under += 1;
        }
        // A click on a companion's portrait, the pointer free (in play the
        // mouse looks, the cursor held); on its pazaak icon, watching its
        // player's match instead; on the weapon swap button, a swap; on
        // the minimap, the map page.
        let free = cursors.iter().next().is_some_and(|cursor| cursor.visible);
        if let Some(point) = input
            .pointer
            .filter(|_| input.click && free && !ui.stack.is_open())
        {
            match hud_click(ui, (size.x, size.y), point) {
                Some(HudClick::Watch(id)) => watch.icon = Some(id),
                Some(HudClick::Portrait(id)) => picked.0 = Some(id),
                Some(HudClick::Swap) => swap.0 = true,
                Some(HudClick::Map) => {
                    ui.stack.open("map", &ui.vm);
                }
                None => {}
            }
        }
    }
    // A conversation's screen under the screens, the HUD and barks given
    // way to it; the host's request card over both.
    let in_play = presented.is_some();
    let dialog = in_play
        .then(|| dialog_layer(ui, (size.x, size.y)))
        .flatten();
    let card = in_play.then(|| card_layer(ui, (size.x, size.y))).flatten();
    for layer in [card, dialog].into_iter().flatten() {
        layers.insert(under, layer);
    }
    // The status summary over the HUD, under the screens.
    if let Some(summaries) = presented
        .is_some()
        .then(|| summary_layer(ui, (size.x, size.y)))
        .flatten()
        .filter(|_| !talking && !minigame && !faded)
    {
        layers.insert(under, summaries);
    }
    // A bark over the HUD (a minigame's too), under the screens.
    if let Some(bark) = presented
        .is_some()
        .then(|| bark_layer(ui, (size.x, size.y)))
        .flatten()
        .filter(|_| !talking)
    {
        layers.insert(under, bark);
    }
    // A terminal's camera feed: its look over the view, under the screens.
    if presented.is_some() && ui.vm.camera_feed {
        layers.insert(
            under,
            ui::screens::feed::draw((size.x, size.y), ui.stack.clock()),
        );
    }
    // A break's or ready check's banner over everything: the screens, a
    // conversation, a watching player's view.
    if let Some(banner) = presented
        .is_some()
        .then(|| hud_layer(ui, (size.x, size.y), true))
        .flatten()
    {
        layers.push(banner);
    }
    ui.layers = layers;
    open.0 = ui.stack.is_open();
    states.0.retain(|key, _| !key.starts_with("ui_"));
    for screen in ui.stack.screens() {
        states.0.insert(format!("ui_{}", screen.0), true);
    }
    states.0.insert("ui".into(), open.0);
}

/// KOTOR's HUD layout placed for a window of `window` pixels, and where it
/// was read from, when it reads.
fn hud_gui(ui: &mut Ui, window: (f32, f32)) -> Option<(fmt_odyssey::gui::Gui, ui::skin::Source)> {
    let assets = ui.assets.as_mut()?;
    let (mut gui, source) = ui
        .hud
        .get_or_insert_with(|| {
            let (mut gui, source) = assets.screen_layout(ui::screens::hud::LAYOUT).ok()?;
            if let Ok(campaign) = assets.layout(ui::screens::hud::LAYOUT) {
                ui::screens::hud::backed(&mut gui, &campaign);
            }
            Some((gui, source))
        })
        .clone()?;
    ui::screens::hud::anchor(&mut gui, window);
    Some((gui, source))
}

/// What a click on the HUD is on.
enum HudClick {
    /// A companion's pazaak icon (`hud::watch_at`), by its body.
    Watch(u32),
    /// A companion's portrait (`hud::portrait_at`), by its body.
    Portrait(u32),
    /// The weapon swap button (`hud::swap_at`).
    Swap,
    /// The minimap (`hud::minimap_at`): the map page.
    Map,
}

/// What the click at `point` (window pixels) on the HUD is on, if anything.
fn hud_click(ui: &mut Ui, window: (f32, f32), point: [f32; 2]) -> Option<HudClick> {
    use ui::screens::hud;
    let (gui, _) = hud_gui(ui, window)?;
    let (width, height) = gui.frame();
    let (scale, [left, top]) = fit(window, (width as f32, height as f32));
    let at = [(point[0] - left) / scale, (point[1] - top) / scale];
    let vm = &ui.vm;
    hud::watch_at(&gui, vm, at)
        .map(HudClick::Watch)
        .or_else(|| hud::portrait_at(&gui, vm, at).map(HudClick::Portrait))
        .or_else(|| hud::swap_at(&gui, vm, at).then_some(HudClick::Swap))
        .or_else(|| hud::minimap_at(&gui, vm, at).then_some(HudClick::Map))
}

/// KOTOR's HUD for a window of `window` pixels, when the game is in play
/// and its layout reads; with `banner`, only its banner (a break, a ready
/// check: `hud::banner`), when there is one.
fn hud_layer(ui: &mut Ui, window: (f32, f32), banner: bool) -> Option<DrawList> {
    if banner {
        ui.vm.calls.banner.as_ref()?;
    } else {
        ui.vm.hud.as_ref()?;
    }
    let (gui, source) = hud_gui(ui, window)?;
    let Ui {
        assets, vm, stack, ..
    } = ui;
    let assets = assets.as_mut()?;
    let mut out = DrawList::default();
    // The minimap's picture under the layout (its border), the arrow over.
    let minimap = vm
        .hud
        .as_ref()
        .and_then(|hud| hud.minimap.as_ref())
        .filter(|_| !banner)
        .and_then(|view| ui::screens::hud::minimap(&gui, view, assets, source));
    out.quads
        .extend(minimap.iter().flat_map(ui::screens::hud::Minimap::under));
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    painter.source = source;
    painter.pulsing = stack.pulsing();
    painter.layout(
        &gui,
        &|control| {
            if banner {
                ui::screens::hud::banner(control, vm)
            } else {
                ui::screens::hud::state(control, vm)
            }
        },
        &mut out,
    );
    out.quads.extend(minimap.and_then(|minimap| minimap.arrow));
    if !banner {
        ui::screens::hud::busy(&gui, vm, &mut painter, &mut out);
        ui::screens::hud::action_keys(&gui, vm, &mut painter, &mut out);
    }
    if vm.crosshair {
        out.quads.extend(ui::screens::hud::crosshair(&gui));
    }
    Some(out)
}

/// The words floating over creatures for a window of `window` pixels,
/// while there are any (`ui::screens::floaty`).
fn floaty_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets, vm, stack, ..
    } = ui;
    if vm.floaties.is_empty() {
        return None;
    }
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    Some(ui::screens::floaty::draw(
        &mut painter,
        &vm.floaties,
        window,
    ))
}

/// The Force power carousel for a window of `window` pixels, while the
/// player picks a power (`ui::screens::power_pick`).
fn power_pick_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets, vm, stack, ..
    } = ui;
    let pick = vm.power_pick.as_ref()?;
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    Some(ui::screens::power_pick::draw(&mut painter, pick, window))
}

/// The combat arena's line of keys for a window of `window` pixels, in
/// the arena (`crate::arena`).
fn arena_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets, vm, stack, ..
    } = ui;
    let words = vm.arena.as_ref()?;
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    Some(ui::screens::power_pick::line(&mut painter, words, window))
}

/// The status summaries for a window of `window` pixels, level with the
/// bottom of the HUD's minimap, while there are any and the layout reads
/// (`ui::screens::status_summary`).
fn summary_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    if ui.vm.summaries.is_empty() {
        return None;
    }
    let (hud, _) = hud_gui(ui, window)?;
    let (width, height) = hud.frame();
    let (scale, [_, top]) = fit(window, (width as f32, height as f32));
    let below = hud
        .find("LBL_MAPBORDER")
        .map_or(0.0, |map| (map.extent.top + map.extent.height) as f32);
    let Ui {
        assets,
        vm,
        stack,
        summary,
        ..
    } = ui;
    let assets = assets.as_mut()?;
    let (gui, source) = summary
        .get_or_insert_with(|| {
            assets
                .screen_layout(ui::screens::status_summary::LAYOUT)
                .ok()
        })
        .as_ref()?;
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    Some(ui::screens::status_summary::draw(
        &mut painter,
        gui,
        *source,
        &vm.summaries,
        window,
        top + below * scale + 12.0,
    ))
}

/// The bark bubble for a window of `window` pixels, while there is a bark
/// and its layout reads (`ui::screens::bark`).
fn bark_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets,
        bark,
        vm,
        stack,
        ..
    } = ui;
    let said = vm.bark.as_ref()?;
    let assets = assets.as_mut()?;
    let bubble = bark
        .get_or_insert_with(|| ui::screens::bark::Bubble::read(assets))
        .as_ref()?;
    let gui = ui::screens::bark::place(&bubble.gui, assets, bubble.source, said, window)?;
    let mut out = DrawList {
        fill: true,
        ..DrawList::default()
    };
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    painter.source = bubble.source;
    painter.layout(
        &gui,
        &|control| ui::screens::bark::state(control, said, bubble.words),
        &mut out,
    );
    Some(out)
}

/// The conversation screen for a window of `window` pixels, while this
/// player is in a cinematic conversation (`ui::screens::dialog`).
fn dialog_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let placed = ui.dialog_placed(window)?;
    let Ui {
        assets,
        dialog,
        vm,
        stack,
        ..
    } = ui;
    let (view, assets) = (vm.dialog.as_ref()?, assets.as_mut()?);
    let source = dialog.as_ref()?.as_ref()?.source;
    let mut out = DrawList {
        fill: true,
        ..DrawList::default()
    };
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    placed.draw(view, source, &mut painter, &mut out);
    Some(out)
}

/// The host's request card for a window of `window` pixels while a guest
/// asks it in (`ui::screens::dialog::Dialog::card`), whether the host
/// talks (under its top bar) or walks.
fn card_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let talking = ui.dialog_placed(window);
    let Ui {
        assets,
        dialog,
        vm,
        stack,
        ..
    } = ui;
    let (requests, assets) = (vm.requests.as_ref()?, assets.as_mut()?);
    let dialog = dialog
        .get_or_insert_with(|| ui::screens::dialog::Dialog::read(assets))
        .as_ref()?;
    let card = dialog.card(assets, requests, window, talking.as_ref())?;
    let mut out = DrawList::default();
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    card.draw(dialog.source, &mut painter, &mut out);
    Some(out)
}

/// The screens' keys in a game: Escape (or gamepad Start) opens the menu
/// at its options; I, J, M, P and K open their screens, or switch the
/// open tab to theirs, or close theirs.
fn in_game(
    ui: &mut Ui,
    input: &mut ui::input::UiInput,
    keys: Option<&ButtonInput<KeyCode>>,
    map: &crate::settings::KeyMap,
) {
    use ui::input::Key;
    if !ui.stack.is_open() && input.keys.contains(&Key::Back) {
        input.keys.retain(|key| *key != Key::Back);
        ui.stack.open("options_in_game", &ui.vm);
    }
    let Some(keys) = keys else {
        return;
    };
    for (action, name) in HOTKEYS {
        if !map.pressed_now(keys, action) {
            continue;
        }
        let tab = |screen: &str| {
            ui::screens::tabs::TABS
                .iter()
                .any(|(_, _, tab)| *tab == screen)
        };
        match ui.stack.top() {
            None => {
                ui.stack.open(name, &ui.vm);
            }
            Some(top) if top.0 == name => ui.stack.clear(),
            Some(top) if tab(top.0) && tab(name) => {
                ui.stack.switch(name, &ui.vm);
            }
            Some(_) => {}
        }
    }
}

/// The screens a key opens in a game.
const HOTKEYS: [(crate::settings::Action, &str); 5] = {
    use crate::settings::Action;
    [
        (Action::Inventory, "inventory"),
        (Action::Journal, "journal"),
        (Action::Map, "map"),
        (Action::Party, "party"),
        (Action::Abilities, "abilities"),
    ]
};

/// The player down closes every screen once (`Stack::fell`), before a
/// script's panel for the fall opens; the whole party down (I30 P13) holds
/// the lost box open on every seat, the host's with Load, until the world
/// is no longer lost.
pub fn fall(
    mut ui: ResMut<Ui>,
    presented: Option<Res<crate::sim_host::PresentedSnapshot>>,
    own: Option<Res<crate::sim_host::OwnSeat>>,
) {
    let ui = &mut *ui;
    let (down, lost) = presented
        .zip(own.as_ref())
        .map_or((false, false), |(presented, own)| {
            let snapshot = presented.0.current();
            (dead(snapshot, own.0), snapshot.lost.is_some())
        });
    ui.stack.fell(down || lost);
    // The host is seat 0 (I30 P4).
    ui.vm.lost = lost.then(|| ui::vm::LostView {
        host: own.is_some_and(|own| own.0 == sim::Seat(0)),
    });
    ui.stack.sync("lost", lost, &ui.vm);
}

/// `seat`'s body is down or dead (a seat that watches is not).
fn dead(snapshot: &sim::Snapshot, seat: sim::Seat) -> bool {
    snapshot
        .body(seat)
        .and_then(|entity| entity.fighter.as_ref())
        .is_some_and(|fighter| !fighter.alive())
}

/// A quad's corners in the window (its layer's layout placed in `size`,
/// `DrawList::placed`) and its texture coordinates, after its clip and
/// turn; `None` when nothing of it shows.
fn corners(quad: &Quad, layer: &DrawList, size: Vec2) -> Option<[([f32; 2], [f32; 2]); 4]> {
    let [rx, ry, rw, rh] = quad.rect;
    if rw <= 0.0 || rh <= 0.0 {
        return None;
    }
    let [cx, cy, cw, ch] = quad.clip.unwrap_or(quad.rect);
    let (left, top) = (rx.max(cx), ry.max(cy));
    let (right, bottom) = ((rx + rw).min(cx + cw), (ry + rh).min(cy + ch));
    if right <= left || bottom <= top {
        return None;
    }
    let (scale, [ox, oy]) = layer.placed((size.x, size.y));
    let place = |x: f32, y: f32| {
        [
            -size.x / 2.0 + ox + x * scale,
            size.y / 2.0 - (oy + y * scale),
        ]
    };
    let centre = [rx + rw / 2.0, ry + rh / 2.0];
    let at = |x: f32, y: f32| {
        let (u, v) = texel(quad, (x - rx) / rw, (y - ry) / rh);
        let [x, y] = turn([x, y], centre, quad.spin);
        (place(x, y), [u, v])
    };
    Some([
        at(left, top),
        at(right, top),
        at(right, bottom),
        at(left, bottom),
    ])
}

/// A texture with its mipmaps (each half the last, averaged), so a screen
/// drawn smaller than its layout reads them (`ui.wgsl`).
fn upload(assets: &GuiAssets, id: TextureId, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let pixels = assets.pixels(id)?;
    let mut image = Image::new_uninit(
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mips = mipmaps(pixels.width, pixels.height, &pixels.rgba);
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.concat());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    Some(images.add(image))
}

/// `rgba` (`width` × `height`) and each level below it down to one texel
/// across, each texel the average of the two by two above it.
fn mipmaps(width: u32, height: u32, rgba: &[u8]) -> Vec<Vec<u8>> {
    let mut levels = vec![rgba.to_vec()];
    let (mut w, mut h) = (width.max(1), height.max(1));
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let above = levels.last().expect("the first level");
        let mut level = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        sum += u32::from(above[((sy * w + sx) * 4 + c) as usize]);
                    }
                    level[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        levels.push(level);
        (w, h) = (nw, nh);
    }
    levels
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    ui: Res<Ui>,
    mut drawn: ResMut<Drawn>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scenes: ResMut<crate::gui_scene::GuiScenes>,
    scene: Query<Option<&RenderTarget>, (With<Camera>, Without<OverlayCamera>)>,
    own: Query<&Camera, With<UiCamera>>,
    mut projections: Query<&mut Projection, With<ScreensCamera>>,
    mut visibility: Query<&mut Visibility>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let drawn = &mut *drawn;
    facts.0.insert(
        "ui_drawn".into(),
        serde_json::json!({
            "camera": drawn.camera.is_some(),
            "size": [drawn.size.x, drawn.size.y],
            "runs": drawn.runs.len(),
            "images": drawn.images.len(),
        }),
    );
    if drawn.camera.is_none() {
        let Some(target) = scene.iter().next() else {
            return;
        };
        let image = images.add(Image::new_target_texture(
            1,
            1,
            TextureFormat::Rgba8Unorm,
            None,
        ));
        let screens = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 12,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                Projection::Orthographic(OrthographicProjection::default_3d()),
                Tonemapping::None,
                DebandDither::Disabled,
                Transform::from_xyz(0.0, 0.0, 100.0),
                RenderLayers::layer(LAYER),
                OverlayCamera,
                ScreensCamera,
            ))
            .id();
        let mut camera = commands.spawn((
            Camera3d::default(),
            crate::scene::over_world(13),
            crate::scene::encoded_view(),
            RenderLayers::none(),
            OverlayCamera,
            UiCamera,
            UiComposite::new(image.clone()),
        ));
        if let Some(target) = target {
            camera.insert(target.clone());
        }
        drawn.camera = Some(camera.id());
        drawn.screens = Some((screens, image));
        return;
    }
    let Some((size, pixels)) = own.iter().next().and_then(|camera| {
        Some((
            camera.logical_viewport_size()?,
            camera.physical_viewport_size()?,
        ))
    }) else {
        return;
    };
    // The screens' image the window's pixels, laid out in its points.
    if let Some((screens, image)) = &drawn.screens {
        if let Some(mut image) = images.get_mut(image)
            && (image.width(), image.height()) != (pixels.x, pixels.y)
        {
            image.resize(Extent3d {
                width: pixels.x.max(1),
                height: pixels.y.max(1),
                depth_or_array_layers: 1,
            });
        }
        if let Ok(mut projection) = projections.get_mut(*screens)
            && let Projection::Orthographic(ortho) = &mut *projection
            && !matches!(ortho.scaling_mode,
                ScalingMode::Fixed { width, height } if (width, height) == (size.x, size.y))
        {
            ortho.scaling_mode = ScalingMode::Fixed {
                width: size.x,
                height: size.y,
            };
        }
    }
    // The textures the assets let go, let go here too.
    if let Some(assets) = &ui.assets
        && assets.released() != drawn.released
    {
        drawn.released = assets.released();
        let kept = |id: &TextureId| {
            *id == TextureId::WHITE
                || assets.scene_of(*id).is_some()
                || assets.pixels(*id).is_some()
        };
        drawn.images.retain(|id, _| kept(id));
        drawn.materials.retain(|(id, _), _| kept(id));
    }
    if drawn.layers == ui.layers && drawn.size == size {
        return;
    }
    drawn.layers = ui.layers.clone();
    drawn.size = size;
    let Some(assets) = &ui.assets else {
        return;
    };
    let mut runs: Vec<Run> = Vec::new();
    for layer in &ui.layers {
        for quad in &layer.quads {
            let Some(corners) = corners(quad, layer, size) else {
                continue;
            };
            let key = (quad.texture, quad.additive);
            if runs.last().is_none_or(|run| run.key != key) {
                runs.push(Run {
                    key,
                    positions: Vec::new(),
                    uvs: Vec::new(),
                    colors: Vec::new(),
                });
            }
            let z = runs.len() as f32 * 0.01;
            let color = quad.color;
            let run = runs.last_mut().expect("pushed above");
            for (position, uv) in corners {
                run.positions.push([position[0], position[1], z]);
                run.uvs.push(uv);
                run.colors.push(color);
            }
        }
    }
    let count = runs.len();
    for (at, run) in runs.into_iter().enumerate() {
        let Run {
            key,
            positions,
            uvs,
            colors,
        } = run;
        let geometry = (positions, uvs, colors);
        if !drawn.materials.contains_key(&key) {
            let scene = assets.scene_of(key.0).is_some();
            let texture = if key.0 == TextureId::WHITE {
                None
            } else if scene {
                Some(scenes.image(key.0, &mut images))
            } else {
                if !drawn.images.contains_key(&key.0)
                    && let Some(image) = upload(assets, key.0, &mut images)
                {
                    drawn.images.insert(key.0, image);
                }
                drawn.images.get(&key.0).cloned()
            };
            let pixels = (key.0 != TextureId::WHITE)
                .then(|| assets.pixels(key.0))
                .flatten();
            let material = materials.add(UiMaterial {
                params: UiParams {
                    size: pixels.map_or(Vec2::ONE, |pixels| {
                        Vec2::new(pixels.width as f32, pixels.height as f32)
                    }),
                    shape: u32::from(pixels.is_some_and(|pixels| pixels.shape)),
                    view: u32::from(scene),
                },
                texture,
                additive: key.1,
            });
            drawn.materials.insert(key, material);
        }
        let material = drawn.materials[&key].clone();
        match drawn.runs.get_mut(at) {
            Some(run) => {
                if run.geometry != geometry
                    && let Some(mut slot) = meshes.get_mut(&run.mesh)
                {
                    *slot = run_mesh(&geometry);
                    run.geometry = geometry;
                }
                if run.key != key {
                    commands.entity(run.entity).insert(MeshMaterial3d(material));
                    run.key = key;
                }
                if !run.shown
                    && let Ok(mut shown) = visibility.get_mut(run.entity)
                {
                    *shown = Visibility::Inherited;
                    run.shown = true;
                }
            }
            None => {
                let handle = meshes.add(run_mesh(&geometry));
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        RenderLayers::layer(LAYER),
                    ))
                    .id();
                drawn.runs.push(Shown {
                    entity,
                    mesh: handle,
                    geometry,
                    key,
                    shown: true,
                });
            }
        }
    }
    for run in drawn.runs.iter_mut().skip(count).filter(|run| run.shown) {
        if let Ok(mut shown) = visibility.get_mut(run.entity) {
            *shown = Visibility::Hidden;
            run.shown = false;
        }
    }
}
