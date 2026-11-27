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
/// as play opens it, `steps` taken in turn (`press TAG`, `row TAG N`, `type TEXT`, `scroll N`;
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
    let result = Game::from_key(game)
        .ok_or(format!("{game}: use k1 or tsl"))
        .and_then(|game| {
            let file = file.map_or_else(|| format!("{}.png", name.replace(':', "_")), str::to_string);
            let path = output(game, out)?.join(file);
            render_screen(discovery, (game, name), (steps, error, None), (size, 1.0), &path)
        });
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

/// The screen `name` of `game` drawn as `screen` draws it, at `size`
/// physical pixels with `factor` of them to each logical pixel, into the
/// PNG at `path` with its control tree next to it. With `capture`, the
/// screen shows the capture of play of that name.
pub(crate) fn render_screen(
    discovery: &Discovery,
    (game, name): (Game, &str),
    (steps, error, capture): (&[String], Option<String>, Option<&str>),
    (size, factor): ((u32, u32), f32),
    path: &Path,
) -> Result<PathBuf, String> {
    let mut staged = kjk_game::story_stage::stage(
        discovery,
        (game, name),
        (steps, error, capture),
        (size, factor),
    )?;
    let shown_layers = staged.frame();
    let kjk_game::story_stage::Staged {
        assets,
        vm,
        stack,
        quiet,
        ..
    } = &mut staged;
    let mut image = vec![0u8; (size.0 * size.1 * 4) as usize];
    for layer in &shown_layers {
        let drawn = raster(&layer.list, &|id| assets.pixels(id), size.0, size.1, [0, 0, 0, 0]);
        over(&mut image, &drawn);
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    save(path, &image, size.0, size.1)?;
    let links = stack.focus_links(vm);
    let focus = stack.focused().map(str::to_string);
    let back = back(stack, assets, vm, quiet);
    let layers: Vec<kjk_game::gui_tree::Layer<'_>> = (shown_layers.iter())
        .map(|layer| kjk_game::gui_tree::Layer {
            screen: &layer.screen,
            layout: &layer.layout,
            list: &layer.list,
        })
        .collect();
    let mut shown = serde_json::json!({
        "game": game.key(),
        "screen": name,
        "steps": steps,
        "scale_factor": factor,
    });
    if let Some(capture) = capture {
        shown["capture"] = capture.into();
    }
    let tree = kjk_game::gui_tree::tree(assets, shown, &layers, size, (&links, focus.as_deref()), back);
    write_tree(path, &tree)?;
    Ok(path.to_path_buf())
}

/// What Back does on the top screen of `stack`: the screens open after
/// it, top last, and the intents it raised. It is taken last, as it
/// changes the stack.
fn back(
    stack: &mut ui::screen::Stack,
    assets: &mut GuiAssets,
    vm: &ui::vm::ViewModel,
    quiet: &ui::input::UiInput,
) -> serde_json::Value {
    let pressed = ui::input::UiInput {
        keys: vec![ui::input::Key::Back],
        ..quiet.clone()
    };
    let frame = stack.step(assets, vm, &pressed);
    let intents: Vec<String> = frame.intents.iter().map(|intent| format!("{intent:?}")).collect();
    let screens: Vec<&str> = stack.screens().map(|id| id.0).collect();
    serde_json::json!({ "screens": screens, "intents": intents })
}

/// `tree` written as JSON next to the picture at `png`.
fn write_tree(png: &Path, tree: &serde_json::Value) -> Result<(), String> {
    let path = png.with_extension("json");
    let text = serde_json::to_string_pretty(tree).map_err(|error| error.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) use kjk_game::gui_tree::over;

pub fn sweep(discovery: &Discovery, game: Option<&str>, out: Option<PathBuf>) -> ExitCode {
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
                if let Some(out) = &out {
                    match drawn_at_rest(discovery, game, &out.join(game.key())) {
                        Ok(count) => outln!("  {count} layouts drawn into {}", out.display()),
                        Err(error) => {
                            outln!("  not drawn: {error}");
                            ok = false;
                        }
                    }
                }
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

/// Every layout of `game` drawn at rest at its own size into `folder`, a
/// PNG and a control tree each; how many were drawn.
fn drawn_at_rest(discovery: &Discovery, game: Game, folder: &Path) -> Result<usize, String> {
    let mut assets = GuiAssets::open(discovery, game)?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut count = 0;
    for name in assets.layouts() {
        let Ok(gui) = assets.layout(&name) else {
            continue;
        };
        let mut list = DrawList::default();
        let mut painter = Painter::new(&mut assets, palette(game));
        painter.marked = true;
        painter.layout(&gui, &|_| ui::paint::ControlState::default(), &mut list);
        let size = (list.size.0 as u32, list.size.1 as u32);
        let path = folder.join(format!("{name}.png"));
        save(&path, &draw(&assets, &list, size.0, size.1), size.0, size.1)?;
        let links = ui::screen::layout_links(&gui);
        let layer = kjk_game::gui_tree::Layer {
            screen: &name,
            layout: &name,
            list: &list,
        };
        let shown = serde_json::json!({ "game": game.key(), "layout": name });
        let tree = kjk_game::gui_tree::tree(
            &assets,
            shown,
            &[layer],
            size,
            (&links, None),
            serde_json::Value::Null,
        );
        write_tree(&path, &tree)?;
        count += 1;
    }
    Ok(count)
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
