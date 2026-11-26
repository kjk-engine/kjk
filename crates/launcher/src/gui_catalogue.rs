//! `kjk gui list`, `kjk gui show` and `kjk gui story`: the UI catalogue
//! from a shell (`ui::catalogue`, docs/UI.md). Agents use these where a
//! person uses the catalogue in the game.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ui::catalogue::{Input, SIZES, Size, Stories, Story, read_all, slug};
use vfs::{Discovery, Game};

fn stories() -> Result<Vec<Stories>, String> {
    read_all(&ui::catalogue::folder())
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Every module's status and stories: one line each, or JSON.
pub fn list(json: bool) -> ExitCode {
    finish(stories().map(|files| {
        if json {
            let modules: Vec<serde_json::Value> = (files.iter())
                .map(|file| {
                    let stories: Vec<serde_json::Value> = (file.stories.iter())
                        .map(|story| {
                            serde_json::json!({
                                "name": story.name,
                                "slug": slug(&story.name),
                                "screen": story.screen,
                                "games": story.games,
                                "capture": story.capture,
                                "pad_capture": story.pad_capture,
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "module": file.module,
                        "status": file.status,
                        "notes": file.notes,
                        "stories": stories,
                    })
                })
                .collect();
            let sizes: Vec<serde_json::Value> = (SIZES.iter())
                .map(|size| serde_json::json!({ "name": size.name, "pixels": [size.pixels.0, size.pixels.1], "scale_factor": size.factor }))
                .collect();
            let inputs: Vec<&str> = Input::ALL.iter().map(|input| input.name()).collect();
            let out = serde_json::json!({ "modules": modules, "sizes": sizes, "inputs": inputs });
            outln!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|error| error.to_string()));
        } else {
            for file in &files {
                outln!("{} ({}): {} stories", file.module, file.status, file.stories.len());
            }
        }
    }))
}

/// One module's notes and its stories with their steps.
pub fn show(module: &str) -> ExitCode {
    finish(stories().and_then(|files| {
        let file = (files.iter().find(|file| file.module == module))
            .ok_or(format!("{module}: no story file"))?;
        outln!("{} ({})", file.module, file.status);
        if !file.notes.is_empty() {
            outln!("{}", file.notes);
        }
        for story in &file.stories {
            outln!("- {} [{}]: {} on {}", story.name, slug(&story.name), story.screen, story.games.join(", "));
            for step in &story.steps {
                outln!("    {step}");
            }
            if let Some(error) = &story.error {
                outln!("    error: {error}");
            }
            for (input, capture) in [("mouse", &story.capture), ("pad", &story.pad_capture)] {
                if let Some(capture) = capture {
                    outln!("    capture for the {input}: {capture}");
                }
            }
        }
        Ok(())
    }))
}

/// What `story` draws: the module's stories (all, or the one named or
/// slugged `story`), at `size` (all six when none), with `input` (both when
/// none), on `game` (each the story names when none).
pub struct Choice<'a> {
    pub module: &'a str,
    pub story: Option<&'a str>,
    pub size: Option<&'a str>,
    pub input: Option<&'a str>,
    pub game: Option<&'a str>,
}

/// Draws the chosen stories into `out`, by default the catalogue's
/// folder of the artifacts: `<game>/<module>/<story>/<size>-<input>.png`,
/// each with its control tree.
pub fn story(discovery: &Discovery, choice: &Choice<'_>, out: Option<PathBuf>) -> ExitCode {
    let out = out.unwrap_or_else(|| crate::runtime::artifacts().join("gui").join("catalogue"));
    finish((|| {
        let files = stories()?;
        let file = (files.iter().find(|file| file.module == choice.module))
            .ok_or(format!("{}: no story file", choice.module))?;
        let chosen: Vec<&Story> = (file.stories.iter())
            .filter(|story| choice.story.is_none_or(|wanted| story.name == wanted || slug(&story.name) == wanted))
            .collect();
        if chosen.is_empty() {
            return Err(format!("{}: no story {:?}", choice.module, choice.story.unwrap_or("")));
        }
        let sizes: Vec<Size> = match choice.size {
            Some(name) => vec![Size::named(name).ok_or(format!("{name}: no such size"))?],
            None => SIZES.to_vec(),
        };
        let inputs: Vec<Input> = match choice.input {
            Some(name) => vec![Input::named(name).ok_or(format!("{name}: pad or mouse"))?],
            None => Input::ALL.to_vec(),
        };
        let mut drawn = 0;
        for story in chosen {
            for game in &story.games {
                if choice.game.is_some_and(|wanted| wanted != game) {
                    continue;
                }
                let key = Game::from_key(game).ok_or(format!("{game}: use k1 or tsl"))?;

                for size in &sizes {
                    for input in &inputs {
                        let Some(capture) = story.capture_for(*input) else {
                            outln!("{}: no capture for the pad, drawn with the mouse alone", story.name);
                            continue;
                        };

                        let mut steps = input.steps(*size);
                        steps.extend(story.steps.iter().cloned());
                        let path = drawn_path(&out, (game, &file.module, &story.name), *size, *input);
                        crate::gui_verbs::render_screen(
                            discovery,
                            (key, &story.screen),
                            (&steps, story.error.clone(), capture),
                            (size.pixels, size.factor),
                            &path,
                        )?;
                        outln!("{}", path.display());
                        drawn += 1;
                    }
                }
            }
        }
        outln!("{drawn} drawn");
        Ok(())
    })())
}

fn drawn_path(out: &Path, (game, module, story): (&str, &str, &str), size: Size, input: Input) -> PathBuf {
    out.join(game)
        .join(module)
        .join(slug(story))
        .join(format!("{}-{}.png", size.name, input.name()))
}

/// What the layouts of each install use, counted: how many layouts, the
/// controls of each kind, the controls that write in each font and in
/// each colour, and the controls that draw each border texture. It is
/// written to `out` (by default the repository's `data/ui/census.json`),
/// the "before" of the design tokens.
pub fn census(discovery: &Discovery, out: Option<PathBuf>) -> ExitCode {
    use std::collections::BTreeMap;
    use fmt_odyssey::gui::Border;
    let out = out.unwrap_or_else(|| ui::catalogue::root().join("data/ui/census.json"));
    finish((|| {
        let mut games = serde_json::Map::new();
        for game in [Game::Kotor1, Game::Kotor2] {
            let Ok(assets) = ui::assets::GuiAssets::open(discovery, game) else {
                outln!("{}: no install, skipped", game.key());
                continue;
            };
            let mut layouts = 0;
            let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
            let mut fonts: BTreeMap<String, u32> = BTreeMap::new();
            let mut colours: BTreeMap<String, u32> = BTreeMap::new();
            let mut borders: BTreeMap<String, u32> = BTreeMap::new();
            let hex = |rgb: [f32; 3]| {
                let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                format!("#{:02x}{:02x}{:02x}", byte(rgb[0]), byte(rgb[1]), byte(rgb[2]))
            };
            for name in assets.layouts() {
                let Ok(gui) = assets.layout(&name) else {
                    continue;
                };
                layouts += 1;
                for control in gui.walk() {
                    *kinds.entry(format!("{:?}", control.kind).to_lowercase()).or_default() += 1;
                    if let Some(text) = &control.text {
                        *fonts.entry(text.font.to_lowercase()).or_default() += 1;
                        if let Some(colour) = text.color {
                            *colours.entry(hex(colour)).or_default() += 1;
                        }
                    }
                    let frames: [&Option<Border>; 4] =
                        [&control.border, &control.hilight, &control.selected, &control.hilight_selected];
                    for border in frames.into_iter().flatten() {
                        for texture in [&border.corner, &border.edge, &border.fill] {
                            if !texture.is_empty() && texture != "0" {
                                *borders.entry(texture.to_lowercase()).or_default() += 1;
                            }
                        }
                    }
                }
            }
            games.insert(
                game.key().to_string(),
                serde_json::json!({
                    "layouts": layouts,
                    "kinds": kinds,
                    "fonts": fonts,
                    "text_colours": colours,
                    "border_textures": borders,
                }),
            );
        }
        let census = serde_json::json!({
            "about": "What the layouts of each install use, counted by `kjk gui census` (docs/UI.md, \"Design tokens\"). Each number is how many controls use it.",
            "games": games,
        });
        let text = serde_json::to_string_pretty(&census).map_err(|error| error.to_string())?;
        std::fs::write(&out, text + "\n").map_err(|error| format!("{}: {error}", out.display()))?;
        outln!("{}", out.display());
        Ok(())
    })())
}
