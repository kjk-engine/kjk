//! `kjk gui`: KOTOR's layouts drawn without a window. `probes` runs the
//! UI's checks; `render` draws one layout to a PNG; `sweep` paints every
//! layout of each install and counts what does not resolve; `sheet` draws
//! every layout of an install onto one contact sheet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kjk_game::story_stage::Scene;
use ui::assets::GuiAssets;
use ui::draw::DrawList;
use ui::paint::Painter;
use ui::raster::raster;
use ui::render::Render;
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
    let result = Game::from_key(game)
        .ok_or(format!("{game}: use k1 or tsl"))
        .and_then(|game| {
            let file =
                file.map_or_else(|| format!("{}.png", name.replace(':', "_")), str::to_string);
            let path = output(game, out)?.join(file);
            let scene = Scene {
                game,
                screen: name.to_string(),
                steps: steps.to_vec(),
                error,
                capture: None,
            };
            let size = ui::catalogue::Size {
                name: "screen",
                pixels: size,
                factor: 1.0,
            };
            render_screen(discovery, scene, size, &path)
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

/// `scene` drawn as `screen` draws it, at `size`, into the PNG at `path`
/// with its control tree next to it.
pub(crate) fn render_screen(
    discovery: &Discovery,
    scene: Scene,
    size: ui::catalogue::Size,
    path: &Path,
) -> Result<PathBuf, String> {
    let mut staged = kjk_game::story_stage::stage(discovery, scene, size)?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    write(path, &staged.render())?;
    Ok(path.to_path_buf())
}

/// `render`'s picture written to the PNG at `png`, and its control tree
/// next to it as JSON.
fn write(png: &Path, render: &Render<'_>) -> Result<(), String> {
    let (width, height) = render.window();
    save(png, &render.rgba(), width, height)?;
    let path = png.with_extension("json");
    let text = serde_json::to_string_pretty(&render.tree()).map_err(|error| error.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|error| format!("{}: {error}", path.display()))
}

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
        let Ok(render) = Render::at_rest(&mut assets, &name) else {
            continue;
        };
        write(&folder.join(format!("{name}.png")), &render)?;
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
        ui::render::over(&mut sheet, &text);
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
