//! `kjk gui`: KOTOR's layouts drawn without a window. `probes` runs the
//! UI's checks; `render` draws one layout to a PNG; `sweep` paints every
//! layout of each install and counts what does not resolve; `sheet` draws
//! every layout of an install onto one contact sheet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ui::assets::GuiAssets;
use ui::draw::DrawList;
use ui::paint::Painter;
use ui::raster::raster;
use ui::sweep::{paint, palette, sweep_install};
use vfs::{Discovery, Game};

pub fn probes() -> ExitCode {
    let checks = ui::probes::all();
    for check in &checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        outln!("{mark} {}: {}", check.name, check.detail);
    }
    if checks.iter().all(|check| check.ok) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn output(game: Game, out: Option<PathBuf>) -> Result<PathBuf, String> {
    let folder = out.unwrap_or_else(|| crate::runtime::artifacts().join("gui").join(game.key()));
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    Ok(folder)
}

fn save(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn draw(assets: &GuiAssets, list: &DrawList, width: u32, height: u32) -> Vec<u8> {
    raster(list, &|id| assets.pixels(id), width, height, [0, 0, 0, 255])
}

pub fn render(
    discovery: &Discovery,
    game: &str,
    name: &str,
    skinned: bool,
    size: (u32, u32),
    out: Option<PathBuf>,
) -> ExitCode {
    let result = (|| -> Result<PathBuf, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: use k1 or tsl"))?;
        let mut assets = GuiAssets::open(discovery, game)?;
        let (list, refusals) = if skinned {
            assets = crate::runtime::skinned(assets, discovery)?;
            let (gui, source) = assets.screen_layout(name)?;
            outln!("{name}: {source:?}");
            let mut list = DrawList::default();
            let mut painter = Painter::new(&mut assets, palette(game));
            painter.source = source;
            painter.layout(&gui, &|_| ui::paint::ControlState::default(), &mut list);
            (list, painter.refusals)
        } else {
            let gui = assets.layout(name)?;
            paint(&mut assets, &gui)
        };
        for why in &refusals {
            outln!("refused: {why}");
        }
        let path = output(game, out)?.join(format!("{name}.png"));
        save(&path, &draw(&assets, &list, size.0, size.1), size.0, size.1)?;
        Ok(path)
    })();
    match result {
        Ok(path) => {
            outln!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// A screen of `game` (`chargen:solo`, ...) opened on the game's own data,
/// as play opens it, `steps` taken in turn (`press TAG`, `type TEXT`, `scroll N`;
/// `key MOVE` a screen move, `Down`, `Activate`; `raw NAME` a key, mouse
/// button or pad chord pressed, `KeyZ`, `MouseBack`, `Pad:LB+South`; `pad
/// KIND` the pad played with, its hints shown, `xbox`, `playstation`, `nintendo`, `deck`), its
/// layers drawn into one PNG named `file`. The view model is what the game
/// fills for the screen (character creation's choices, the session's
/// `error` for `message`); the 3D views are not drawn.
pub fn screen(
    discovery: &Discovery,
    (game, name): (&str, &str),
    (steps, error): (&[String], Option<String>),
    size: (u32, u32),
    (out, file): (Option<PathBuf>, Option<&str>),
) -> ExitCode {
    let result = (|| -> Result<PathBuf, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: use k1 or tsl"))?;
        // The data as play opens it: KOTOR 1's with KOTOR 2's powers and
        // feats lent.
        let install = kjk_game::installs::open(discovery, game)?;
        let assets = GuiAssets::with(game, install.index.clone(), install.base()?.clone());
        let mut assets = crate::runtime::skinned(assets, discovery)?;
        // The settings as play shows them (`KJK_SETTINGS` names another).
        let mut vm = ui::vm::ViewModel {
            settings: kjk_game::settings::Settings::load().values_shown(),
            movie_duels: discovery
                .get(Game::JediAcademy)
                .is_some_and(vfs::Install::movie_duels),
            ..ui::vm::ViewModel::default()
        };
        vm.session.error = error;
        (vm.keys, vm.pad_keys) = kjk_game::key_rows();
        let padded = |vm: &mut ui::vm::ViewModel, kind: kjk_game::pad::PadKind| {
            vm.pad_kind = kind.key().to_string();
            vm.pad_names = kind.names();
        };
        padded(&mut vm, kjk_game::pad::PadKind::Xbox);
        if matches!(name.split(':').next(), Some("chargen" | "levelup")) {
            vm.chargen = Some(crate::character::chargen(&mut assets));
        }
        if matches!(name, "workbench" | "chemical") {
            vm.workbench = Some(stand_in_bench(&mut assets, name == "chemical"));
        }
        if name == "character" {
            vm.character = Some(stand_in_member());
            let folders = kjk_game::stance_sets::offered_folders(discovery).unwrap_or_else(|why| {
                outln!("no stance sets offered: {why}");
                Vec::new()
            });
            vm.stances = folders
                .into_iter()
                .map(|folder| {
                    let title = kjk_game::stance_sets::title(&folder);
                    (folder, title)
                })
                .collect();
        }
        if name == "lobby" {
            vm.lobby = Some(stand_in_lobby(&mut assets, game, 0, 0));
        }
        let mut stack = ui::screen::Stack::new(ui::screens::open);
        if !stack.open(name, &vm) {
            return Err(format!("{name}: no such screen"));
        }
        let quiet = ui::input::UiInput {
            window: (size.0 as f32, size.1 as f32),
            dt: 0.016,
            ..ui::input::UiInput::default()
        };
        stack.step(&mut assets, &vm, &quiet);
        for step in steps {
            match step.split_once(' ') {
                Some(("press", tag)) => stack.press(tag),
                Some(("set", setting)) => {
                    let (key, value) = setting
                        .split_once(' ')
                        .ok_or(format!("{step}: set KEY VALUE"))?;
                    vm.settings.insert(key.to_string(), value.to_string());
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("session", rules)) => {
                    vm.session_rules = Some(rules.to_string());
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("lobby", "guest")) => {
                    vm.lobby = Some(stand_in_lobby(&mut assets, game, 2, 0));
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("lobby", "hero")) => {
                    vm.lobby = Some(stand_in_lobby(&mut assets, game, 0, 1));
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("lobby", "hero guest")) => {
                    vm.lobby = Some(stand_in_lobby(&mut assets, game, 1, 1));
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("lobby", "droid")) => {
                    vm.lobby = Some(droid_lobby(&mut assets, game));
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("lobby", lobby))
                    if lobby.starts_with("save ") || lobby.starts_with("checkpoint ") =>
                {
                    let id = match lobby.strip_prefix("checkpoint ") {
                        Some(path) => checkpoint_save(game, path)?,
                        None => lobby["save ".len()..].to_string(),
                    };
                    let chosen = kjk_game::lobby_host::choose(discovery, Some(id), None, None)?;
                    let mut lobby = kjk_game::lobby_host::lobby_of(&chosen);
                    lobby.join("Mia".into());
                    lobby.join("Kalle".into());
                    outln!("lobby facts {}", lobby_facts(&chosen, &lobby));
                    vm.lobby = Some(ui::vm::LobbyView {
                        address: "192.168.1.20 : 7777".into(),
                        steam: true,
                        ..kjk_game::lobby_host::view(&lobby, 0)
                    });
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("stands", folder)) => {
                    if let Some(me) = vm.character.as_mut() {
                        me.stance = Some(kjk_game::stance_sets::set_title(folder));
                    }
                    stack.step(&mut assets, &vm, &quiet);
                }
                Some(("type", text)) => {
                    let typed = ui::input::UiInput {
                        text: text.to_string(),
                        ..quiet.clone()
                    };
                    stack.step(&mut assets, &vm, &typed);
                }
                Some(("pad", kind)) => {
                    let kind = kjk_game::pad::PadKind::named(kind)
                        .ok_or(format!("{kind}: xbox, playstation, nintendo or deck"))?;
                    padded(&mut vm, kind);
                    vm.pad_playing = true;
                }
                Some(("raw", pressed)) => {
                    let raw = ui::input::UiInput {
                        raw: vec![pressed.to_string()],
                        ..quiet.clone()
                    };
                    let frame = stack.step(&mut assets, &vm, &raw);
                    for intent in frame.intents {
                        outln!("{step}: {intent:?}");
                    }
                }
                Some(("key", moved)) => {
                    use ui::input::Key;
                    let key = [
                        ("Up", Key::Up),
                        ("Down", Key::Down),
                        ("Left", Key::Left),
                        ("Right", Key::Right),
                        ("Activate", Key::Activate),
                        ("Back", Key::Back),
                        ("Primary", Key::Primary),
                    ]
                    .into_iter()
                    .find(|(name, _)| *name == moved)
                    .ok_or(format!("{moved}: Up, Down, Left, Right, Activate, Back or Primary"))?
                    .1;
                    let keyed = ui::input::UiInput {
                        keys: vec![key],
                        ..quiet.clone()
                    };
                    let frame = stack.step(&mut assets, &vm, &keyed);
                    for intent in frame.intents {
                        outln!("{step}: {intent:?}");
                    }
                }
                Some(("scroll", notches)) => {
                    let notches: f32 = notches
                        .parse()
                        .map_err(|_| format!("{step:?}: `scroll N`, N notches down"))?;
                    let wheeled = ui::input::UiInput {
                        pointer: Some([size.0 as f32 / 2.0, size.1 as f32 / 2.0]),
                        moved: true,
                        wheel: -notches,
                        ..quiet.clone()
                    };
                    stack.step(&mut assets, &vm, &wheeled);
                }
                _ => {
                    return Err(format!(
                        "{step:?}: `press TAG`, `set KEY VALUE`, `session RULES`, `stands FOLDER`, `type TEXT`, `scroll N`, `key MOVE`, `raw NAME` or `pad KIND`"
                    ));
                }
            }
            let frame = stack.step(&mut assets, &vm, &quiet);
            for intent in frame.intents {
                outln!("{step}: {intent:?}");
            }
        }
        let frame = stack.step(&mut assets, &vm, &quiet);
        for why in &frame.refusals {
            outln!("refused: {why}");
        }
        let mut image = vec![0u8; (size.0 * size.1 * 4) as usize];
        for layer in &frame.layers {
            let drawn = raster(layer, &|id| assets.pixels(id), size.0, size.1, [0, 0, 0, 0]);
            over(&mut image, &drawn);
        }
        let file = file.map_or_else(|| format!("{}.png", name.replace(':', "_")), str::to_string);
        let path = output(game, out)?.join(file);
        save(&path, &image, size.0, size.1)?;
        Ok(path)
    })();
    match result {
        Ok(path) => {
            outln!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The save id the `lobby checkpoint PATH` step opens: the checkpoint at
/// `path` written into the saves folder (`KJK_SAVES`, a scratch folder in a
/// check) as the game's checkpoint save 99 (`tsl-99`), named by the
/// checkpoint's file name, so the lobby reads a copy.
fn checkpoint_save(game: Game, path: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = (path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .ok_or(format!("{}: no file name", path.display()))?;
    let key = if game == Game::Kotor2 { "tsl" } else { "k1" };
    let id = saves::checkpoint_id(key, 99);
    let root = crate::save_view::folder().ok_or("no folder for saves here")?;
    crate::chain::write_checkpoint(&root, &id, &bytes, (&name, &name), saves::now())?;
    Ok(id)
}

/// What the lobby on a real save shows, as one line of JSON for the lobby's
/// checks (`cargo xtask lobby --sections`): the story's character in the
/// main character's place, each companion offered and its choice, what each
/// slot plays, how the save's party table marks the part of the story (the
/// companion the first seat is switched to, the companion leading, the
/// party's members and its puppets), the main character (`main_facts`),
/// and the companion leading once Start has written the lobby's party.
fn lobby_facts(chosen: &kjk_game::lobby_host::Chosen, lobby: &session::lobby::Lobby) -> String {
    let text = |bytes: &[u8]| fmt_odyssey::code_page(bytes);
    let summary = &chosen.summary;
    let party = chosen.save.as_ref().map(|save| &save.script.party);
    let companions: Vec<serde_json::Value> = (lobby.companions.iter())
        .map(|companion| {
            serde_json::json!({
                "npc": companion.npc,
                "name": text(&companion.name),
                "choice": format!("{:?}", companion.choice),
            })
        })
        .collect();
    let slots: Vec<String> = (lobby.slots.iter())
        .map(|slot| slot.as_ref().map_or("empty".into(), |slot| format!("{:?}", slot.playing)))
        .collect();
    serde_json::json!({
        "save": text(&summary.save),
        "hero": text(&summary.hero),
        "hero_portrait": summary.hero_portrait,
        "hero_class": text(&summary.hero_class),
        "hero_level": summary.hero_level,
        "companions": companions,
        "slots": slots,
        "party": lobby.party(),
        "switched": party.and_then(|party| party.switched),
        "leader": party.and_then(|party| party.leader),
        "members": party.map(|party| party.members.clone()),
        "puppets": party.map(|party| party.puppets.keys().copied().collect::<Vec<u8>>()),
        "main": chosen.save.as_ref().map(main_facts),
        "start_leader": (chosen.save.as_ref())
            .and_then(|save| kjk_game::lobby_host::started(save, lobby).ok())
            .map(|started| started.script.party.leader),
    })
    .to_string()
}

/// The main character as `save` holds it: whether its avatar is there,
/// whether it can be played (`lobby_host::main_playable`), and how far it
/// stands from the companion leading the party.
fn main_facts(save: &sim::Save) -> serde_json::Value {
    let snapshot = &save.snapshot;
    let own = snapshot.own(sim::Seat(0));
    let leader = (save.script.party.leader)
        .and_then(|npc| save.script.party.available.get(&npc)?.object)
        .and_then(|id| snapshot.get(sim::SimId(id)));
    let apart = own.zip(leader).map(|(own, leader)| {
        (0..3)
            .map(|axis| (own.position[axis] - leader.position[axis]).powi(2))
            .sum::<f32>()
            .sqrt()
    });
    serde_json::json!({
        "present": own.is_some(),
        "playable": kjk_game::lobby_host::main_playable(save),
        "apart_m": apart,
    })
}

/// The companions a stand-in lobby lists, by party number and template, in
/// each game's party selection order.
const LOBBY_K1: &[(u8, &str)] = &[
    (0, "p_bastilla"),
    (1, "p_cand"),
    (2, "p_carth"),
    (3, "p_hk47"),
    (4, "p_jolee"),
    (5, "p_juhani"),
    (6, "p_mission"),
    (7, "p_t3m4"),
    (8, "p_zaalbar"),
];
const LOBBY_TSL: &[(u8, &str)] = &[
    (0, "p_atton"),
    (1, "p_baodur"),
    (2, "p_mand"),
    (3, "p_g0t0"),
    (4, "p_handmaiden"),
    (5, "p_hk47"),
    (6, "p_kreia"),
    (7, "p_mira"),
    (8, "p_t3m4"),
    (9, "p_visas"),
];

/// A lobby the lobby screen shows (`kjk gui screen <game> lobby`, the
/// `lobby guest` step for a guest's view, and the `lobby hero` and `lobby
/// hero guest` steps where the first guest plays the main character): the
/// game's companions by its own names and portraits, two guests who chose
/// the third and first, the fifth taken away by the story, the main
/// character played by slot `hero`, seen from slot `own`.
fn stand_in_lobby(
    assets: &mut GuiAssets,
    game: Game,
    own: usize,
    hero: usize,
) -> ui::vm::LobbyView {
    use session::lobby::{Collision, Companion, Game as Played, Lobby, Summary};
    use sim::script::party_choice::Choice;
    let roster = if game == Game::Kotor2 { LOBBY_TSL } else { LOBBY_K1 };
    let class = |assets: &mut GuiAssets, row: usize| {
        assets.cell_text("classes", row, "name").unwrap_or_else(Vec::new)
    };
    let companions = (roster.iter().enumerate())
        .map(|(at, &(npc, template))| Companion {
            npc,
            name: (assets.template_name(template, fmt_odyssey::ResType::UTC))
                .unwrap_or_else(|| template.as_bytes().to_vec()),
            portrait: assets.portrait_of(template, 0).unwrap_or_else(String::new),
            class: class(assets, at % 3),
            level: 4,
            choice: if at == 4 { Choice::Unavailable } else { Choice::Open },
        })
        .collect();
    let summary = Summary {
        save: b"Endar Spire".to_vec(),
        area: b"Command Module".to_vec(),
        game: if game == Game::Kotor2 { Played::Tsl } else { Played::K1 },
        saved: 1_794_700_800,
        hero: b"Dax Tyrell".to_vec(),
        hero_class: class(assets, 0),
        hero_level: 4,
        // The game's first male player portrait (`portraits.2da`).
        hero_portrait: (0..assets.rows("portraits"))
            .find(|row| {
                (assets.cell("portraits", *row, "baseresref"))
                    .is_some_and(|name| name.to_ascii_lowercase().starts_with("po_pm"))
            })
            .and_then(|row| assets.portrait(row, 0))
            .unwrap_or_else(String::new),
    };
    let current = [roster[2].0, roster[0].0];
    let mut lobby = Lobby::new("Host".into(), "ja".into(), Collision::Limited);
    lobby.reset(summary, companions, &current);
    lobby.join("Mia".into());
    lobby.join("Kalle".into());
    lobby.choose(hero, session::lobby::Pick::Hero, true);
    lobby.ready(2, true);
    for slot in lobby.slots.iter_mut().skip(1).flatten() {
        slot.delay_ms = 32;
    }
    ui::vm::LobbyView {
        host: own == 0,
        address: "192.168.1.20 : 7777".into(),
        steam: own == 0,
        ..kjk_game::lobby_host::view(&lobby, own)
    }
}

/// A lobby whose save's party has one companion and no other recruited
/// (the Endar Spire with Trask): the first guest plays him and the second
/// flies a remote droid, seen from the second guest's slot.
fn droid_lobby(assets: &mut GuiAssets, game: Game) -> ui::vm::LobbyView {
    let mut view = stand_in_lobby(assets, game, 2, 0);
    view.companions.truncate(1);
    view.companions[0].state = ui::vm::LobbyCompanionState::Played {
        slot: 1,
        player: "Mia".into(),
    };
    if let Some(Some(slot)) = view.slots.get_mut(1) {
        slot.character = view.companions[0].name.clone();
        slot.portrait = view.companions[0].portrait.clone();
    }
    if let Some(Some(slot)) = view.slots.get_mut(2) {
        slot.character = ui::strings::bytes(ui::strings::LOBBY_DROID);
        slot.class_level.clear();
        slot.portrait = None;
        slot.droid = true;
    }
    if let Some(save) = view.save.as_mut() {
        save.party.truncate(2);
        save.party[1] = view.companions[0].name.clone();
    }
    view
}

/// The member the character page shows: the first seat's character, whose
/// stance may be picked, with level 1 and scores of 10.
fn stand_in_member() -> ui::vm::CharacterView {
    ui::vm::CharacterView {
        stance_setting: Some(kjk_game::stance_sets::pick_setting("seat0")),
        level: 1,
        abilities: [10; 6],
        ..ui::vm::CharacterView::default()
    }
}

/// What the bench draws (`workbench_view::of`): TSL's prologue's table
/// (`tutitemcreate.2da`, what its garage workbench opens), or the lab
/// station's (`lab`), 12 components (chemicals), a stand-in maker with 4
/// ranks of each skill, carrying two blasters, a vibroblade and medpacs.
fn stand_in_bench(assets: &mut GuiAssets, lab: bool) -> ui::vm::WorkbenchView {
    let source = crate::viewer::ResolverSource(assets.resolver());
    let shop = session::workshop::workshop(&source);
    let list = if lab {
        sim::inventory::workshop::LAB
    } else {
        "tutitemcreate"
    };
    let bench = sim::notice::Bench {
        item: None,
        maker: 0,
        upgrade: false,
        list: list.into(),
        lab,
        stock: 12,
    };
    let sheet = sim::combat::Sheet {
        skills: [4; 8],
        ..sim::combat::Sheet::commoner()
    };
    let item = |template: &str, stack| sim::inventory::Item {
        template: template.into(),
        holder: sim::inventory::PARTY,
        slot: None,
        stack,
        charges: 0,
        droppable: true,
        plot: false,
        identified: true,
        infinite: false,
        non_equippable: false,
        upgrades: 0,
        fitted: Vec::new(),
    };
    let carried = [
        item("w_blaste_03", 2),
        item("w_melee_02", 1),
        item("g_i_medeqpmnt01", 3),
    ];
    let carried: Vec<(u32, &sim::inventory::Item)> = carried
        .iter()
        .enumerate()
        .map(|(at, item)| (sim::inventory::FIRST_ITEM + at as u32, item))
        .collect();
    crate::workbench_view::of(assets, (&bench, &shop), (&sheet, &sheet), None, (&carried, false))
}

/// `top`, drawn over nothing (its colour already carries its alpha), laid
/// over `image`, which ends opaque.
fn over(image: &mut [u8], top: &[u8]) {
    for (pixel, drawn) in image
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(top.as_chunks::<4>().0)
    {
        let alpha = f32::from(drawn[3]) / 255.0;
        for k in 0..3 {
            pixel[k] = (f32::from(drawn[k]) + f32::from(pixel[k]) * (1.0 - alpha)).min(255.0) as u8;
        }
        pixel[3] = 255;
    }
}

pub fn sweep(discovery: &Discovery, game: Option<&str>) -> ExitCode {
    let games: Vec<Game> = match game {
        Some(key) => match Game::from_key(key) {
            Some(game) => vec![game],
            None => {
                eprintln!("{key}: use k1 or tsl");
                return ExitCode::FAILURE;
            }
        },
        None => vec![Game::Kotor1, Game::Kotor2],
    };
    let mut ok = true;
    for game in games {
        match sweep_install(discovery, game) {
            Ok(sweep) => {
                outln!(
                    "{}: {} layouts, {} failed to parse, {} unknown control kinds",
                    game.key(),
                    sweep.layouts,
                    sweep.failed.len(),
                    sweep.unknown_kinds.len()
                );
                for failure in &sweep.failed {
                    outln!("  parse: {failure}");
                }
                for kind in &sweep.unknown_kinds {
                    outln!("  kind: {kind}");
                }
                for (kind, list) in &sweep.refusals {
                    outln!("  {} {kind} refusals", list.len());
                    for item in list {
                        outln!("    {item}");
                    }
                }
                outln!("  roots off 0,0: {:?}", sweep.offset_roots);
                outln!("  unread fields: {:?}", sweep.unread);
                ok &= sweep.passes(game);
            }
            Err(error) => outln!("{}: skipped ({error})", game.key()),
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every layout of `game` at `THUMB` each, in rows of `COLUMNS`, each
/// named under it in the game's own font.
pub fn sheet(discovery: &Discovery, game: &str, out: Option<PathBuf>) -> ExitCode {
    const THUMB: (u32, u32) = (320, 240);
    const LABEL: u32 = 18;
    const COLUMNS: u32 = 8;
    let result = (|| -> Result<PathBuf, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: use k1 or tsl"))?;
        let mut assets = GuiAssets::open(discovery, game)?;
        let names = assets.layouts();
        let rows = (names.len() as u32).div_ceil(COLUMNS);
        let (width, height) = (THUMB.0 * COLUMNS, (THUMB.1 + LABEL) * rows);
        let mut sheet = vec![0u8; (width * height * 4) as usize];
        let mut labels = DrawList {
            size: (width as f32, height as f32),
            ..DrawList::default()
        };
        for (at, name) in names.iter().enumerate() {
            let (column, row) = (at as u32 % COLUMNS, at as u32 / COLUMNS);
            let (x, y) = (column * THUMB.0, row * (THUMB.1 + LABEL));
            if let Ok(gui) = assets.layout(name) {
                let (list, _) = paint(&mut assets, &gui);
                let thumb = draw(&assets, &list, THUMB.0, THUMB.1);
                for line in 0..THUMB.1 {
                    let from = (line * THUMB.0 * 4) as usize;
                    let to = (((y + line) * width + x) * 4) as usize;
                    sheet[to..to + (THUMB.0 * 4) as usize]
                        .copy_from_slice(&thumb[from..from + (THUMB.0 * 4) as usize]);
                }
            }
            let mut painter = Painter::new(&mut assets, palette(game));
            painter.text(
                "dialogfont10x10",
                name.as_bytes(),
                [x as f32, (y + THUMB.1) as f32, THUMB.0 as f32, LABEL as f32],
                18,
                [1.0; 4],
                &mut labels,
            );
        }
        let text = raster(
            &labels,
            &|id| assets.pixels(id),
            width,
            height,
            [0, 0, 0, 0],
        );
        over(&mut sheet, &text);
        let path = output(game, out)?.join("sheet.png");
        save(&path, &sheet, width, height)?;
        Ok(path)
    })();
    match result {
        Ok(path) => {
            outln!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
