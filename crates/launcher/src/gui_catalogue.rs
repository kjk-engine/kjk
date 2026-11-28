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

fn by_key(
    captures: &std::collections::BTreeMap<Game, String>,
) -> std::collections::BTreeMap<&str, &str> {
    (captures.iter())
        .map(|(game, name)| (game.key(), name.as_str()))
        .collect()
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

/// Every module's status and stories: one line each, or JSON. What breaks
/// the catalogue's rules (`ui::catalogue::check`) is listed after them, and
/// fails the verb.
pub fn list(json: bool) -> ExitCode {
    let problems: Vec<String> = ui::catalogue::check()
        .iter()
        .map(ToString::to_string)
        .collect();
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
                                "games": story.games.iter().map(|game| game.key()).collect::<Vec<_>>(),
                                "capture": by_key(&story.capture),
                                "pad_capture": by_key(&story.pad_capture),
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "module": file.module,
                        "status": file.status.name(),
                        "notes": file.notes,
                        "stories": stories,
                    })
                })
                .collect();
            let sizes: Vec<serde_json::Value> = (SIZES.iter())
                .map(|size| serde_json::json!({ "name": size.name, "pixels": [size.pixels.0, size.pixels.1], "scale_factor": size.factor }))
                .collect();
            let inputs: Vec<&str> = Input::ALL.iter().map(|input| input.name()).collect();
            let out = serde_json::json!({ "modules": modules, "sizes": sizes, "inputs": inputs, "problems": problems });
            outln!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|error| error.to_string()));
        } else {
            for file in &files {
                outln!("{} ({}): {} stories", file.module, file.status, file.stories.len());
            }
            for problem in &problems {
                outln!("problem: {problem}");
            }
        }
    }).and_then(|()| match problems.len() {
        0 => Ok(()),
        count => Err(format!("{count} problems in the catalogue")),
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
            let games: Vec<&str> = story.games.iter().map(|game| game.key()).collect();
            outln!(
                "- {} [{}]: {} on {}",
                story.name,
                slug(&story.name),
                story.screen,
                games.join(", ")
            );
            for step in &story.steps {
                outln!("    {step}");
            }
            if let Some(error) = &story.error {
                outln!("    error: {error}");
            }
            for (input, captures) in [("mouse", &story.capture), ("pad", &story.pad_capture)] {
                for (game, capture) in captures {
                    outln!("    capture on {} for the {input}: {capture}", game.key());
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
    pub size: Option<Size>,
    pub input: Option<Input>,
    pub game: Option<Game>,
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
            .filter(|story| {
                choice
                    .story
                    .is_none_or(|wanted| story.name == wanted || slug(&story.name) == wanted)
            })
            .collect();
        if chosen.is_empty() {
            return Err(format!(
                "{}: no story {:?}",
                choice.module,
                choice.story.unwrap_or("")
            ));
        }
        let sizes: Vec<Size> = choice.size.map_or(SIZES.to_vec(), |size| vec![size]);
        let inputs: Vec<Input> = choice
            .input
            .map_or(Input::ALL.to_vec(), |input| vec![input]);
        let mut drawn = 0;
        for story in chosen {
            for &game in &story.games {
                if choice.game.is_some_and(|wanted| wanted != game) {
                    continue;
                }
                for size in &sizes {
                    for input in &inputs {
                        let Some(capture) = story.capture_for(game, *input) else {
                            outln!(
                                "{}: no capture on {} with the {}",
                                story.name,
                                game.key(),
                                input.name()
                            );
                            continue;
                        };

                        let mut steps = input.steps(*size);
                        steps.extend(story.steps.iter().cloned());
                        let scene = kjk_game::story_stage::Scene {
                            game,
                            screen: story.screen.clone(),
                            steps,
                            error: story.error.clone(),
                            capture: capture.map(str::to_string),
                        };
                        let path =
                            drawn_path(&out, (game, &file.module, &story.name), *size, *input);
                        crate::gui_verbs::render_screen(discovery, scene, *size, &path)?;
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

fn drawn_path(
    out: &Path,
    (game, module, story): (Game, &str, &str),
    size: Size,
    input: Input,
) -> PathBuf {
    out.join(game.key())
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
    use fmt_odyssey::gui::Border;
    use std::collections::BTreeMap;
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
                format!(
                    "#{:02x}{:02x}{:02x}",
                    byte(rgb[0]),
                    byte(rgb[1]),
                    byte(rgb[2])
                )
            };
            for name in assets.layouts() {
                let Ok(gui) = assets.layout(&name) else {
                    continue;
                };
                layouts += 1;
                for control in gui.walk() {
                    *kinds
                        .entry(format!("{:?}", control.kind).to_lowercase())
                        .or_default() += 1;
                    if let Some(text) = &control.text {
                        *fonts.entry(text.font.to_lowercase()).or_default() += 1;
                        if let Some(colour) = text.color {
                            *colours.entry(hex(colour)).or_default() += 1;
                        }
                    }
                    let frames: [&Option<Border>; 4] = [
                        &control.border,
                        &control.hilight,
                        &control.selected,
                        &control.hilight_selected,
                    ];
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

/// Checks that no capture of play in the checkout holds the words of its
/// game's talk table (docs/UI.md, "Captures of play"). The words of the
/// other game's talk table that a capture holds are listed as notes, and
/// do not fail the check: kjk's own words, such as the floors stage's
/// "TSL Malachor V", may be lines there. With `rewrite`, each capture is
/// first written again from what it holds, read and written with its own
/// game's talk table, so that it keeps only what `Capture::of` keeps today.
pub fn captures(discovery: &Discovery, rewrite: bool) -> ExitCode {
    use ui::catalogue::{Capture, TalkTable, game_words, read_capture, write_capture};
    finish((|| {
        let folder = ui::catalogue::captures();
        let tlks: Vec<(Game, std::sync::Arc<fmt_odyssey::tlk::Tlk>)> = [Game::Kotor1, Game::Kotor2]
            .into_iter()
            .filter_map(|game| {
                let install = kjk_game::installs::open(discovery, game).ok()?;
                Some((game, install.base().ok()?.talk.get()?))
            })
            .collect();
        if tlks.is_empty() {
            return Err("no game is installed to read a talk table from".into());
        }
        let talks: Vec<(Game, TalkTable<'_>)> = (tlks.iter())
            .map(|(game, tlk)| (*game, TalkTable::new(tlk)))
            .collect();
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
            .collect();
        names.sort();
        let mut found = 0;
        for name in &names {
            let path = folder.join(format!("{name}.json"));
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let key = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| Some(value.get("game")?.as_str()?.to_string()))
                .ok_or(format!("{name}: names no game"))?;
            if rewrite {
                let (game, talk) = (talks.iter())
                    .find(|(game, _)| game.key() == key)
                    .ok_or(format!("{name}: {key} is not installed"))?;
                let old = read_capture(&folder, name, talk)?;
                let new = Capture::of(&old.view, *game, old.window, old.scale_factor, &old.screens);
                write_capture(&folder, name, &new, talk).map_err(|why| format!("{name}: {why}"))?;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            for (game, talk) in &talks {
                let own = game.key() == key;
                for words in game_words(&text, talk).map_err(|why| format!("{name}: {why}"))? {
                    if own {
                        outln!("{name}: {} words at {words}", game.key());
                        found += 1;
                    } else {
                        outln!(
                            "{name}: note, a line of {}'s talk table at {words}",
                            game.key()
                        );
                    }
                }
            }
        }
        outln!(
            "{} captures, {found} fields with the game's words",
            names.len()
        );
        if found > 0 {
            return Err(format!(
                "{found} fields of the captures hold the game's words"
            ));
        }
        Ok(())
    })())
}
