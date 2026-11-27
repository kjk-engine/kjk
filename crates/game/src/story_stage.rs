//! A screen opened on stand-in data, or on a capture of play, and taken
//! through a story's steps (`kjk gui screen --step`, docs/UI.md, "UI
//! catalogue"). The shell's catalogue verbs draw what this stages, and the
//! in-game catalogue will show it.

use ui::assets::GuiAssets;
use ui::draw::DrawList;
use vfs::{Discovery, Game};

/// A screen as a story left it: the stack it is open in, the view model
/// it is drawn from, and the assets it draws with.
pub struct Staged {
    pub game: Game,
    pub assets: GuiAssets,
    pub vm: ui::vm::ViewModel,
    pub stack: ui::screen::Stack,
    /// A frame of input with nothing pressed, in the stack's window.
    pub quiet: ui::input::UiInput,
    /// Where the pointer is: the window's middle until `point` moves it.
    pointer: [f32; 2],
    /// What is drawn over play, for the story screen `play`
    /// (`ui::catalogue::PLAY`).
    pub over: Option<ui::over_play::OverPlay>,
}

/// A staged frame after the owner's input: its layers, and what the screens
/// made of the input.
pub struct Live {
    pub shown: Vec<Shown>,
    pub sounds: Vec<ui::intent::GuiSound>,
    pub intents: Vec<ui::intent::Intent>,
    pub refusals: Vec<String>,
}

/// One layer of a staged frame: the screen module that drew it, the layout
/// it drew, and what it drew.
pub struct Shown {
    pub screen: String,
    pub layout: String,
    pub list: DrawList,
}

/// The screen `name` of `game` opened as play opens it, in a window of
/// `size` physical pixels with `factor` of them to each logical pixel, and
/// `steps` taken in turn. The view model is that of the capture named
/// `capture` (`data/ui/captures`), or else what the game fills for the screen (character creation's choices, the session's
/// `error` for `message`).
pub fn stage(
    discovery: &Discovery,
    (game, name): (Game, &str),
    (steps, error, capture): (&[String], Option<String>, Option<&str>),
    (size, factor): ((u32, u32), f32),
) -> Result<Staged, String> {
    let mut staged = Staged::open(
        discovery,
        (game, name),
        (steps, error, capture),
        (size, factor),
    )?;
    for step in steps {
        staged.step(discovery, step)?;
    }
    Ok(staged)
}

impl Staged {
    /// The screen opened, before any step but `level`, which sets the
    /// member before the screen opens.
    pub fn open(
        discovery: &Discovery,
        (game, name): (Game, &str),
        (steps, error, capture): (&[String], Option<String>, Option<&str>),
        (size, factor): ((u32, u32), f32),
    ) -> Result<Staged, String> {
        // The data as play opens it: KOTOR 1's with KOTOR 2's powers and
        // feats lent.
        let install = crate::installs::open(discovery, game)?;
        let assets = GuiAssets::with(game, install.index.clone(), install.base()?.clone());
        let mut assets = crate::runtime::skinned(assets, discovery)?;
        let vm = match capture {
            Some(name) => {
                let tlk = assets
                    .talk()
                    .ok_or("no talk table to read a capture's words from")?;
                let talk = ui::catalogue::TalkTable::new(tlk);
                ui::catalogue::read_capture(&ui::catalogue::captures(), name, &talk)?.view
            }
            None => stand_ins(discovery, (game, name), (steps, error), &mut assets)?,
        };
        let mut stack = ui::screen::Stack::new(ui::screens::open);
        stack.marked = true;
        let play = name == ui::catalogue::PLAY;
        if !play && !stack.open(name, &vm) {
            return Err(format!("{name}: no such screen"));
        }
        let window = (size.0 as f32 / factor, size.1 as f32 / factor);
        let quiet = ui::input::UiInput {
            window,
            scale_factor: factor,
            dt: 0.016,
            ..ui::input::UiInput::default()
        };
        stack.step(&mut assets, &vm, &quiet);
        Ok(Staged {
            game,
            assets,
            vm,
            stack,
            quiet,
            pointer: [window.0 / 2.0, window.1 / 2.0],
            over: play.then(ui::over_play::OverPlay::default),
        })
    }

    /// The frame as it stands, bottom layer first: what is drawn over play,
    /// then the screens of the stack. What a screen refused to draw is
    /// printed.
    pub fn frame(&mut self) -> Vec<Shown> {
        let quiet = self.quiet.clone();
        let frame = self.live(&quiet);
        for why in &frame.refusals {
            outln!("refused: {why}");
        }
        frame.shown
    }

    /// The frame after `input`, which the catalogue in the game gives it:
    /// what the owner's mouse, keys and pad did, in the stack's window.
    pub fn live(&mut self, input: &ui::input::UiInput) -> Live {
        let frame = self.stack.step(&mut self.assets, &self.vm, input);
        let mut shown = Vec::new();
        if let Some(over) = self.over.as_mut() {
            let mut drawing = ui::hud_layer::Frame {
                assets: &mut self.assets,
                vm: &self.vm,
                palette: self.stack.palette,
                pulse: self.stack.pulsing(),
                scale_factor: input.scale_factor,
                settings: 0,
                marked: self.stack.marked,
            };
            let corner = ui::draw::kjk_option(&self.vm.settings, ui::draw::CORNER_NOTICES);
            let still = over.still(&mut drawing, input.window, (self.stack.clock(), corner));
            for layer in still {
                shown.push(Shown {
                    screen: layer.module.to_string(),
                    layout: layer.layout.to_string(),
                    list: layer.list,
                });
            }
        }
        let screens = self.stack.screens().map(|id| id.0.to_string());
        let layouts = self.stack.layouts().map(str::to_string);
        for ((screen, layout), list) in screens.zip(layouts).zip(frame.layers) {
            shown.push(Shown {
                screen,
                layout,
                list,
            });
        }
        Live {
            shown,
            sounds: frame.sounds,
            intents: frame.intents,
            refusals: frame.refusals,
        }
    }

    /// One step taken (`press TAG`, `row TAG N` a click on row N of a list,
    /// from 0, `type TEXT`, `scroll N`; `hover TAG` the
    /// pointer onto the middle of a control; `key MOVE` a
    /// screen move, `Down`, `Activate`; `raw NAME` a key, mouse button or
    /// pad chord pressed, `KeyZ`, `MouseBack`, `Pad:LB+South`; `pad KIND`
    /// the pad played with, its hints shown, `xbox`, `playstation`,
    /// `nintendo`, `deck`), and a quiet frame after it.
    pub fn step(&mut self, discovery: &Discovery, step: &str) -> Result<(), String> {
        let Staged {
            game,
            assets,
            vm,
            stack,
            quiet,
            pointer,
            ..
        } = self;
        let (game, quiet) = (*game, &*quiet);
        match step.split_once(' ') {
            Some(("press", tag)) => stack.press(tag),
            Some(("row", at)) => {
                let (tag, row) = at
                    .rsplit_once(' ')
                    .and_then(|(tag, row)| Some((tag, row.parse::<usize>().ok()?)))
                    .ok_or(format!("{step:?}: `row TAG N`, N the row from 0"))?;
                stack.click_row(tag, row);
            }
            Some(("set", setting)) => {
                let (key, value) = setting
                    .split_once(' ')
                    .ok_or(format!("{step}: set KEY VALUE"))?;
                vm.settings.insert(key.to_string(), value.to_string());
                stack.step(assets, vm, quiet);
            }
            Some(("session", rules)) => {
                vm.session_rules = Some(rules.to_string());
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", "guest")) => {
                vm.lobby = Some(stand_in_lobby(assets, game, 2, 0, false));
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", "hero")) => {
                vm.lobby = Some(stand_in_lobby(assets, game, 0, 1, false));
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", "hero guest")) => {
                vm.lobby = Some(stand_in_lobby(assets, game, 1, 1, false));
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", "all")) => {
                vm.lobby = Some(stand_in_lobby(assets, game, 0, 0, true));
                stack.step(assets, vm, quiet);
            }
            Some(("party", "all")) => {
                vm.party_selection = Some(stand_in_party(assets, game, true));
                stack.step(assets, vm, quiet);
            }
            Some(("pazaak", change)) => {
                let table = vm.pazaak.as_mut().ok_or("pazaak: on pazaak_game only")?;
                match change.split_once(' ') {
                    Some(("turn", whose)) => table.turn = whose.parse().ok(),
                    Some(("played", nth)) => {
                        let nth: usize = nth.parse().map_err(|_| "pazaak played N")?;
                        if let Some(held) = table.sides[0].hand.get_mut(nth) {
                            *held = None;
                        }
                    }
                    _ => return Err(format!("{step}: `pazaak turn N` or `pazaak played N`")),
                }
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", "droid")) => {
                vm.lobby = Some(droid_lobby(assets, game));
                stack.step(assets, vm, quiet);
            }
            Some(("lobby", lobby))
                if lobby.starts_with("save ") || lobby.starts_with("checkpoint ") =>
            {
                let id = match lobby.strip_prefix("checkpoint ") {
                    Some(path) => checkpoint_save(game, path)?,
                    None => lobby["save ".len()..].to_string(),
                };
                let chosen = crate::lobby_host::choose(discovery, Some(id), None, None)?;
                let mut lobby = crate::lobby_host::lobby_of(&chosen);
                lobby.join("Mia".into());
                lobby.join("Kalle".into());
                outln!("lobby facts {}", lobby_facts(&chosen, &lobby));
                vm.lobby = Some(ui::vm::LobbyView {
                    address: "192.168.1.20 : 7777".into(),
                    steam: true,
                    ..crate::lobby_host::view(&lobby, 0)
                });
                stack.step(assets, vm, quiet);
            }
            Some(("wheel", what)) => wheel_step(vm, what)?,
            // A terminal's camera feed over play, as the game turns it on
            // while a computer conversation shows a camera.
            Some(("feed", "on")) => vm.camera_feed = true,
            Some(("level", _)) => {}
            Some(("stands", folder)) => {
                if let Some(me) = vm.character.as_mut() {
                    me.stance = Some(crate::stance_sets::set_title(folder));
                }
                stack.step(assets, vm, quiet);
            }
            Some(("type", text)) => {
                let typed = ui::input::UiInput {
                    text: text.to_string(),
                    ..quiet.clone()
                };
                stack.step(assets, vm, &typed);
            }
            Some(("pad", kind)) => {
                let kind = crate::pad::PadKind::named(kind)
                    .ok_or(format!("{kind}: xbox, playstation, nintendo or deck"))?;
                repad(vm, kind.key())?;
                padded(vm, kind);
                vm.pad_playing = true;
            }
            Some(("raw", pressed)) => {
                let raw = ui::input::UiInput {
                    raw: vec![pressed.to_string()],
                    ..quiet.clone()
                };
                let frame = stack.step(assets, vm, &raw);
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
                    ("Second", Key::Second),
                    ("Accept", Key::Accept),
                    ("NextTab", Key::NextTab),
                    ("PrevTab", Key::PrevTab),
                    ("NextPage", Key::NextPage),
                    ("PrevPage", Key::PrevPage),
                ]
                .into_iter()
                .find(|(name, _)| *name == moved)
                .ok_or(format!(
                    "{moved}: Up, Down, Left, Right, Activate, Back, Primary, Second, Accept, NextTab, PrevTab, NextPage or PrevPage"
                ))?
                .1;
                let keyed = ui::input::UiInput {
                    keys: vec![key],
                    ..quiet.clone()
                };
                let frame = stack.step(assets, vm, &keyed);
                for intent in frame.intents {
                    outln!("{step}: {intent:?}");
                }
                // Where the move left the focus, so that a check can
                // follow a pad's player across the screen.
                outln!("{step}: focus {}", stack.focused().unwrap_or("none"));
            }
            Some(("point", at)) => {
                let parsed: Vec<f32> = at.split(' ').filter_map(|n| n.parse().ok()).collect();
                let [x, y] = parsed[..] else {
                    return Err(format!("{step:?}: `point X Y`, a pixel of the frame"));
                };
                *pointer = [x, y];
                let moved = ui::input::UiInput {
                    pointer: Some(*pointer),
                    moved: true,
                    ..quiet.clone()
                };
                stack.step(assets, vm, &moved);
            }
            Some(("hover", tag)) => {
                let frame = stack.step(assets, vm, quiet);
                let list = frame
                    .layers
                    .last()
                    .ok_or(format!("{step:?}: no screen is open"))?;
                let mark = (list.marks.iter().rev())
                    .find(|mark| mark.tag == tag)
                    .ok_or(format!("{step:?}: the top screen has no control {tag}"))?;
                let (scale, [x, y]) = list.placed(quiet.window);
                let [left, top, width, height] = mark.rect;
                *pointer = [
                    x + (left + width / 2.0) * scale,
                    y + (top + height / 2.0) * scale,
                ];
                let moved = ui::input::UiInput {
                    pointer: Some(*pointer),
                    moved: true,
                    ..quiet.clone()
                };
                stack.step(assets, vm, &moved);
            }
            Some(("scroll", notches)) => {
                let notches: f32 = notches
                    .parse()
                    .map_err(|_| format!("{step:?}: `scroll N`, N notches down"))?;
                let wheeled = ui::input::UiInput {
                    pointer: Some(*pointer),
                    moved: true,
                    wheel: -notches,
                    ..quiet.clone()
                };
                stack.step(assets, vm, &wheeled);
            }
            _ => {
                return Err(format!(
                    "{step:?}: `press TAG`, `row TAG N`, `set KEY VALUE`, `session RULES`, `stands FOLDER`, `level CLASS N`, `type TEXT`, `scroll N`, `point X Y`, `hover TAG`, `key MOVE`, `raw NAME`, `pad KIND`, `wheel at N`, `wheel pick NAME`, `wheel close`, `feed on` or `pazaak turn N`"
                ));
            }
        }
        let frame = stack.step(assets, vm, quiet);
        for intent in frame.intents {
            outln!("{step}: {intent:?}");
        }
        Ok(())
    }
}

/// `kind` the pad the view model's hints are drawn for.
fn padded(vm: &mut ui::vm::ViewModel, kind: crate::pad::PadKind) {
    vm.pad_kind = kind.key().to_string();
    vm.pad_names = kind.names();
}

/// The pad glyphs a view model names (`ui::icons::pad_texture`, as a
/// capture taken with a pad holds them on its action bar) renamed for the
/// pad `kind`, so that a capture taken with one pad shows another's buttons.
/// A view model naming no glyph of its pad is left as it is.
fn repad(vm: &mut ui::vm::ViewModel, kind: &str) -> Result<(), String> {
    let was = format!("\"{}{}_", ui::icons::PREFIX, vm.pad_kind);
    if vm.pad_kind == kind {
        return Ok(());
    }
    let text = serde_json::to_string(&*vm).map_err(|error| error.to_string())?;
    if !text.contains(&was) {
        return Ok(());
    }
    let text = text.replace(&was, &format!("\"{}{kind}_", ui::icons::PREFIX));
    *vm = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(())
}

/// A step on an open power wheel: `wheel at N` highlights entry N (from
/// 0), `wheel pick NAME` the first entry whose name holds NAME, and
/// `wheel close` shuts the wheel. The pointer goes to the highlighted
/// entry, the ring turns to show it, and the arc of the entry highlighted
/// before goes away, as only the game knows an entry's arc.
fn wheel_step(vm: &mut ui::vm::ViewModel, what: &str) -> Result<(), String> {
    use ui::screens::wheel::{Ring, ring_slot};
    if what == "close" {
        vm.wheel = None;
        return Ok(());
    }
    let wheel = vm
        .wheel
        .as_mut()
        .ok_or("wheel: no wheel is open (a capture taken with it open has one)")?;
    let at = match what.split_once(' ') {
        Some(("at", nth)) => nth
            .parse()
            .map_err(|_| format!("wheel at {nth}: N is an entry's number"))?,
        Some(("pick", name)) => {
            let name = name.to_lowercase();
            (wheel.entries.iter())
                .position(|entry| {
                    String::from_utf8_lossy(&entry.name)
                        .to_lowercase()
                        .contains(&name)
                })
                .ok_or(format!("wheel pick {name}: no entry is so named"))?
        }
        _ => {
            return Err(format!(
                "wheel {what}: `wheel at N`, `wheel pick NAME` or `wheel close`"
            ));
        }
    };
    let entries = wheel.entries.len();
    if at >= entries {
        return Err(format!("wheel at {at}: the wheel has {entries} entries"));
    }
    if ring_slot(at, wheel.first, entries).is_none() {
        wheel.first = at;
    }
    let groups: Vec<u8> = wheel.entries.iter().map(|entry| entry.group).collect();
    let ring = Ring::shown(&groups, wheel.first);
    let slot = ring_slot(at, wheel.first, entries).ok_or("wheel: the entry is not on the ring")?;
    let angle = ring.angles[slot].to_radians();
    wheel.at = Some(at);
    wheel.pointer = [angle.sin() * ring.radius, angle.cos() * ring.radius];
    wheel.arc.clear();
    wheel.arc_at = None;
    Ok(())
}

/// The view model of the screen `name` of `game` on stand-ins, as play
/// fills it for that screen, with the settings as play shows them
/// (`KJK_SETTINGS` names another).
fn stand_ins(
    discovery: &Discovery,
    (game, name): (Game, &str),
    (steps, error): (&[String], Option<String>),
    assets: &mut GuiAssets,
) -> Result<ui::vm::ViewModel, String> {
    let movie_duels = discovery
        .get(Game::JediAcademy)
        .map_or(vfs::MovieDuels::Absent, vfs::Install::movie_duels_found);
    let mut vm = ui::vm::ViewModel {
        settings: crate::settings::Settings::load().values_shown(),
        movie_duels: movie_duels == vfs::MovieDuels::Installed,
        movie_duels_incomplete: matches!(movie_duels, vfs::MovieDuels::Incomplete { .. }),
        ..ui::vm::ViewModel::default()
    };
    vm.session.error = error;
    (vm.keys, vm.pad_keys) = crate::key_rows();
    padded(&mut vm, crate::pad::PadKind::Xbox);
    if matches!(name.split(':').next(), Some("chargen" | "levelup")) {
        vm.chargen = Some(crate::character::chargen(assets));
    }
    if matches!(name, "workbench" | "chemical") {
        vm.workbench = Some(stand_in_bench(assets, name == "chemical"));
    }
    if name == "character" {
        vm.character = Some(stand_in_member());
        let folders = crate::stance_sets::offered_folders(discovery).unwrap_or_else(|why| {
            outln!("no stance sets offered: {why}");
            Vec::new()
        });
        vm.stances = folders
            .into_iter()
            .map(|folder| {
                let title = crate::stance_sets::title(&folder);
                (folder, title)
            })
            .collect();
    }
    if name == "lobby" {
        vm.lobby = Some(stand_in_lobby(assets, game, 0, 0, false));
    }
    if name == "party" {
        vm.party_selection = Some(stand_in_party(assets, game, false));
    }
    if name == "pazaak_game" {
        vm.pazaak = Some(stand_in_pazaak());
    }
    // The in-game menu of a game played alone, with a party of three,
    // where the player may save and load.
    if ui::screens::tabs::under_tabs(name) {
        vm.here = Some(ui::vm::SaveView::default());
        vm.load_in_play = true;
        vm.party = vec![ui::vm::MemberView::default(); 3];
    }
    for step in steps {
        if let Some(("level", level)) = step.split_once(' ') {
            stand_in_level(&mut vm, assets, level)?;
        }
    }
    Ok(vm)
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
    saves::write_checkpoint(&root, &id, &bytes, (&name, &name), saves::now())?;
    Ok(id)
}

/// What the lobby on a real save shows, as one line of JSON for the lobby's
/// checks (`cargo xtask lobby --sections`): the story's character in the
/// main character's place, each companion offered and its choice, what each
/// slot plays, how the save's party table marks the part of the story (the
/// companion the first seat is switched to, the companion leading, the
/// party's members and its puppets), the main character (`main_facts`),
/// and the companion leading once Start has written the lobby's party.
fn lobby_facts(chosen: &crate::lobby_host::Chosen, lobby: &session::lobby::Lobby) -> String {
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
        .map(|slot| {
            slot.as_ref()
                .map_or("empty".into(), |slot| format!("{:?}", slot.playing))
        })
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
            .and_then(|save| crate::lobby_host::started(save, lobby).ok())
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
        "playable": crate::lobby_host::main_playable(save),
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
/// The two KOTOR 2 companions that take another's place, Hanharr (in
/// Mira's) and the Disciple (in the Handmaiden's). A mod can recruit them
/// beside the companions whose place they take.
const LOBBY_TSL_STAND_INS: &[(u8, &str)] = &[(10, "p_hanharr"), (11, "p_disciple")];

/// The companions a stand-in screen lists: the game's usual ten (nine in
/// KOTOR 1), and with `all` KOTOR 2's stand-ins too, as a mod that recruits
/// every companion makes them.
fn stand_in_roster(game: Game, all: bool) -> Vec<(u8, &'static str)> {
    match game {
        Game::Kotor2 if all => [LOBBY_TSL, LOBBY_TSL_STAND_INS].concat(),
        Game::Kotor2 => LOBBY_TSL.to_vec(),
        _ => LOBBY_K1.to_vec(),
    }
}

/// The table a script's party selection screen shows (`kjk gui screen
/// <game> party`, and the `party all` step for every companion of KOTOR 2
/// available): the game's companions by their own names and portraits,
/// the first and third picked.
fn stand_in_party(assets: &mut GuiAssets, game: Game, all: bool) -> ui::vm::PartySelectionView {
    let roster = stand_in_roster(game, all);
    let slots = if game == Game::Kotor2 { 12 } else { 9 };
    let mut companions = vec![None; slots];
    for &(npc, template) in &roster {
        companions[usize::from(npc)] = Some(ui::vm::CompanionView {
            portrait: assets.portrait_of(template, 0),
            appearance: None,
            armour: None,
            good_evil: None,
            name: (assets.template_name(template, fmt_odyssey::ResType::UTC))
                .unwrap_or_else(|| template.as_bytes().to_vec()),
            selectable: true,
        });
    }
    ui::vm::PartySelectionView {
        slots,
        companions,
        opening: vec![roster[0].0, roster[2].0],
        forced: Vec::new(),
        open: true,
        cancel: true,
        change: ui::vm::PartyChange::May,
    }
}

/// A lobby the lobby screen shows (`kjk gui screen <game> lobby`, the
/// `lobby guest` step for a guest's view, and the `lobby hero` and `lobby
/// hero guest` steps where the first guest plays the main character): the
/// game's companions by its own names and portraits, two guests who chose
/// the third and first, the fifth taken away by the story, the main
/// character played by slot `hero`, seen from slot `own`. With `all` (the
/// `lobby all` step) every companion of KOTOR 2 is recruited, as a mod
/// makes them, and none is taken away.
fn stand_in_lobby(
    assets: &mut GuiAssets,
    game: Game,
    own: usize,
    hero: usize,
    all: bool,
) -> ui::vm::LobbyView {
    use session::lobby::{Collision, Companion, Game as Played, Lobby, Summary};
    use sim::script::party_choice::Choice;
    let roster = stand_in_roster(game, all);
    let class = |assets: &mut GuiAssets, row: usize| {
        assets
            .cell_text("classes", row, "name")
            .unwrap_or_else(Vec::new)
    };
    let companions = (roster.iter().enumerate())
        .map(|(at, &(npc, template))| Companion {
            npc,
            name: (assets.template_name(template, fmt_odyssey::ResType::UTC))
                .unwrap_or_else(|| template.as_bytes().to_vec()),
            portrait: assets.portrait_of(template, 0).unwrap_or_else(String::new),
            class: class(assets, at % 3),
            level: 4,
            choice: if at == 4 && !all {
                Choice::Unavailable
            } else {
                Choice::Open
            },
        })
        .collect();
    let summary = Summary {
        save: b"Endar Spire".to_vec(),
        area: b"Command Module".to_vec(),
        game: if game == Game::Kotor2 {
            Played::Tsl
        } else {
            Played::K1
        },
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
        ..crate::lobby_host::view(&lobby, own)
    }
}

/// A lobby whose save's party has one companion and no other recruited
/// (the Endar Spire with Trask): the first guest plays him and the second
/// flies a remote droid, seen from the second guest's slot.
fn droid_lobby(assets: &mut GuiAssets, game: Game) -> ui::vm::LobbyView {
    let mut view = stand_in_lobby(assets, game, 2, 0, false);
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

/// The table `kjk gui screen <game> pazaak_game` opens on: the player's
/// turn, four ± cards in the player's hand and one in the opponent's. The
/// `pazaak turn N` and `pazaak played N` steps change whose turn it is and
/// take a hand card away.
fn stand_in_pazaak() -> ui::vm::PazaakView {
    use ui::vm::{PazaakCard, PazaakSide, PazaakStage, PazaakView};
    let card = PazaakCard {
        face: "+1".into(),
        art: "lbl_cardrarem".into(),
        signed: Some(["+1", "-1", "+2", "-2"].map(String::from)),
        minus_art: "lbl_cardraref".into(),
        sized: false,
    };
    PazaakView {
        stage: PazaakStage::Game,
        most: 50,
        sides: [
            PazaakSide {
                hand: std::array::from_fn(|_| Some(card.clone())),
                ..PazaakSide::default()
            },
            PazaakSide {
                hand: [Some(card.clone()), None, None, None],
                ..PazaakSide::default()
            },
        ],
        turn: Some(0),
        ..PazaakView::default()
    }
}

/// The member the character page shows: the first seat's character, whose
/// stance may be picked, with level 1 and scores of 10.
fn stand_in_member() -> ui::vm::CharacterView {
    ui::vm::CharacterView {
        stance_setting: Some(crate::stance_sets::pick_setting("seat0")),
        level: 1,
        abilities: [10; 6],
        ..ui::vm::CharacterView::default()
    }
}

/// The member the character page shows and the level-up screen levels
/// (`level CLASS N`): a level `N` character of `CLASS` (a `classes.2da`
/// row) with the class's recommended scores and every feat the class grants
/// up to level `N`, who has the experience for level `N + 1`. The level-up
/// screen offers that level's picks as the game's levelling counts them.
fn stand_in_level(
    vm: &mut ui::vm::ViewModel,
    assets: &mut GuiAssets,
    spec: &str,
) -> Result<(), String> {
    let usage = || format!("{spec:?}: `level CLASS N`, a classes.2da row and a level");
    let (class, level) = spec.split_once(' ').ok_or_else(usage)?;
    let class: u8 = class.parse().map_err(|_| usage())?;
    let level: u8 = level.parse().map_err(|_| usage())?;
    let classes = session::classes::classes(&crate::viewer::ResolverSource(assets.resolver()));
    let of = classes
        .classes
        .get(usize::from(class))
        .ok_or(format!("classes.2da has no row {class}"))?;
    let mut sheet = sim::combat::Sheet {
        level,
        abilities: of.recommended,
        ..sim::combat::Sheet::commoner()
    };
    sheet.feats = Default::default();
    for &(_, feat) in of.granted.iter().filter(|(at, _)| *at <= level) {
        sheet.feats.insert(feat);
    }
    let career = vec![(class, level)];
    let picks = classes.picks(&sheet, &career, class).ok_or(format!(
        "class {class} offers no level {}",
        level.saturating_add(1)
    ))?;
    let name = assets
        .cell_text("classes", usize::from(class), "name")
        .unwrap_or_else(Default::default);
    vm.character = Some(ui::vm::CharacterView {
        class_row: Some(class),
        classes: vec![(name, level)],
        level,
        abilities: of.recommended,
        own_abilities: of.recommended,
        xp: classes.next(level).unwrap_or(0),
        next_xp: classes.next(level),
        may_level: true,
        ..stand_in_member()
    });
    vm.levelup = Some(ui::vm::LevelUpView {
        class,
        level: level.saturating_add(1),
        abilities: of.recommended,
        picks: crate::character::picks_view(assets, &picks),
        ..ui::vm::LevelUpView::default()
    });
    Ok(())
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
    crate::workbench_view::of(
        assets,
        (&bench, &shop),
        (&sheet, &sheet),
        None,
        (&carried, false),
    )
}
