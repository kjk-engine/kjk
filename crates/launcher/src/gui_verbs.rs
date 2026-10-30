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
/// as play opens it, `steps` taken in turn (`press TAG`, `type TEXT`, `scroll N`), its
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
        let mut assets = crate::runtime::skinned(GuiAssets::open(discovery, game)?, discovery)?;
        let mut vm = ui::vm::ViewModel::default();
        vm.session.error = error;
        if matches!(name.split(':').next(), Some("chargen" | "levelup")) {
            vm.chargen = Some(crate::character::chargen(&mut assets));
        }
        if matches!(name, "workbench" | "chemical") {
            vm.workbench = Some(stand_in_bench(&mut assets, name == "chemical"));
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
                Some(("type", text)) => {
                    let typed = ui::input::UiInput {
                        text: text.to_string(),
                        ..quiet.clone()
                    };
                    stack.step(&mut assets, &vm, &typed);
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
                        "{step:?}: `press TAG`, `type TEXT` or `scroll N`"
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
    crate::workbench_view::of(assets, (&bench, &shop), &sheet, (&carried, false))
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
            fill: false,
            quads: Vec::new(),
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
