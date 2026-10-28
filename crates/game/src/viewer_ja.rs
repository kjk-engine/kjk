//! Loads a Jedi Academy Ghoul2 model for the viewer (`kjk view ja <path>`):
//! the GLM, the GLA its header names, the `model_default.skin` beside it,
//! each surface's shader resolved to its first `map` stage, and the
//! textures decoded. Optionally `animation.cfg` sequences to loop.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::{Encoding, FrameStrip, TextureFacts, TextureIr};
use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::{Gla, Glm};
use fmt_ja::pk3::Pk3Entry;
use store::{Key, Store};
use vfs::ja::{JaResolved, JaSource, JaStack, Pk3File};
use vfs::{DirIndex, Discovery, Game};

use crate::viewer::{BladeSpec, EffectBlend, LoadedScene, Placed, SaberColor};

/// JA's files after pk3 precedence.
pub fn stack(discovery: &Discovery) -> Result<JaStack, String> {
    let install = discovery.get(Game::JediAcademy).ok_or(format!(
        "ja: not found (set {})",
        Game::JediAcademy.env_var()
    ))?;
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    Ok(JaStack::open(&index))
}

pub struct Files<'a> {
    stack: &'a JaStack,
    all: BTreeMap<&'a str, JaResolved>,
    /// Where large archive entries are kept unpacked, when it opens.
    store: Option<Store>,
    shaders: OnceLock<BTreeMap<String, ShaderLook>>,
}

/// What a model surface's shader gives its look: the texture of its first
/// `map`/`clampmap` stage that names a file (an animated map's first
/// frame), whether it is drawn from both sides (`cull none`, `disable`
/// or `twosided`; `cull back`, which draws only the back, is drawn from
/// both sides too), and whether that stage clamps its texture
/// (`clampmap`, `clampAnimMap`).
#[derive(Clone, Debug, PartialEq)]
struct ShaderLook {
    texture: String,
    two_sided: bool,
    clamp: bool,
    anim: Option<AnimMap>,
}

/// A stage's animated map (`animMap`, `clampAnimMap`, `oneShotAnimMap`):
/// its frames a second, whether it plays once, and its frames' files (at
/// most eight, as JA's shader parser keeps).
#[derive(Clone, Debug, PartialEq)]
struct AnimMap {
    fps: f32,
    once: bool,
    frames: Vec<String>,
}

/// The most frames JA's shaders keep for an animated map.
const MAX_ANIM_FRAMES: usize = 8;

/// Compressed entries this large and larger are read from the store
/// unpacked (`docs/IMPORT.md`, the JA pack): the skeleton's animations
/// (`_humanoid.gla`, 11.8 MB) took tens of milliseconds to inflate, and
/// hundreds on a busy machine, each time a run read them.
const UNPACKED_FROM: u32 = 1 << 20;

const UNPACKED: &str = "import/ja/unpacked";

impl<'a> Files<'a> {
    pub fn new(stack: &'a JaStack) -> Files<'a> {
        Files {
            stack,
            all: stack.all(),
            store: Store::open(),
            shaders: OnceLock::new(),
        }
    }

    pub(crate) fn read(&self, name: &str) -> Option<Vec<u8>> {
        let name = fmt_ja::game_path(name);
        let found = self.all.get(name.as_str())?;
        if let (Some(store), JaSource::Pk3 { pk3, entry }) = (&self.store, found.source) {
            let pak = &self.stack.pk3s[pk3];
            let entry = &pak.entries[entry];
            if entry.method != 0 && entry.size >= UNPACKED_FROM {
                return unpacked(store, pak, entry);
            }
        }
        self.stack.read(&name, found.source).ok()
    }
}

impl Files<'_> {
    /// What a file holds, cheaply, for keys: an archive entry's CRC and
    /// sizes, a loose file's size and time.
    fn identity(&self, name: &str) -> Vec<u8> {
        let Some(found) = self.all.get(name) else {
            return Vec::new();
        };
        match found.source {
            JaSource::Pk3 { pk3, entry } => {
                let entry = &self.stack.pk3s[pk3].entries[entry];
                [entry.crc, entry.size, entry.compressed]
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect()
            }
            JaSource::Loose => {
                let meta = self
                    .stack
                    .loose
                    .get(name)
                    .and_then(|path| std::fs::metadata(path).ok());
                let modified = meta
                    .as_ref()
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_nanos());
                let mut out = meta.map_or(0, |meta| meta.len()).to_le_bytes().to_vec();
                out.extend(modified.to_le_bytes());
                out
            }
        }
    }
}

/// `entry` of `pak` from the store, else inflated (and kept). Keyed by the
/// archive's name, length and time and the entry's name, sizes and CRC:
/// an archive that changes reads its entries again.
fn unpacked(store: &Store, pak: &Pk3File, entry: &Pk3Entry) -> Option<Vec<u8>> {
    let modified = std::fs::metadata(&pak.path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let key = Key::of(
        "ja-unpacked-v1",
        &[
            pak.name.to_ascii_lowercase().as_bytes(),
            &pak.len.to_le_bytes(),
            &modified.to_le_bytes(),
            entry.name.as_bytes(),
            &entry.crc.to_le_bytes(),
            &entry.size.to_le_bytes(),
            &entry.compressed.to_le_bytes(),
        ],
    );
    if let Some(bytes) = store
        .get(UNPACKED, &key)
        .filter(|bytes| bytes.len() == entry.size as usize)
    {
        return Some(bytes);
    }
    let bytes = pak.read(entry).ok()?;
    let _ = store.put(UNPACKED, &key, &bytes);
    Some(bytes)
}

/// Shader name → its look (`ShaderLook`; a texture is a stage's file, not
/// `$lightmap` or `$whiteimage`): read once a run, and kept in the
/// store, keyed by every shader file's name and contents' CRC and size and
/// this code (`KJK_JA_SHADERS`), as reading them all took tens of
/// milliseconds.
fn shader_textures<'f>(files: &'f Files) -> &'f BTreeMap<String, ShaderLook> {
    files.shaders.get_or_init(|| {
        let names: Vec<&str> = files
            .all
            .keys()
            .copied()
            .filter(|name| name.starts_with("shaders/") && name.ends_with(".shader"))
            .collect();
        let Some(store) = &files.store else {
            return read_shader_textures(files, &names);
        };
        let mut identity = Vec::new();
        for name in &names {
            identity.extend(name.as_bytes());
            identity.push(0);
            identity.extend(files.identity(name));
        }
        let key = Key::of(
            &format!("ja-shader-textures/{}", env!("KJK_JA_SHADERS")),
            &[&identity],
        );
        let kind = "import/ja/shader-textures";
        if let Some(map) = store.get(kind, &key).and_then(|bytes| {
            let text = String::from_utf8(bytes).ok()?;
            text.lines()
                .map(|line| {
                    let (shader, texture, sides, clamp, anim) = match line
                        .split('\t')
                        .collect::<Vec<_>>()[..]
                    {
                        [shader, texture, sides, clamp] => (shader, texture, sides, clamp, None),
                        [shader, texture, sides, clamp, fps, once, frames] => (
                            shader,
                            texture,
                            sides,
                            clamp,
                            Some(AnimMap {
                                fps: fps.parse().ok()?,
                                once: once == "1",
                                frames: frames.split('|').map(str::to_string).collect(),
                            }),
                        ),
                        _ => return None,
                    };
                    Some((
                        shader.to_string(),
                        ShaderLook {
                            texture: texture.to_string(),
                            two_sided: sides == "2",
                            clamp: clamp == "1",
                            anim,
                        },
                    ))
                })
                .collect::<Option<BTreeMap<_, _>>>()
        }) {
            return map;
        }
        let map = read_shader_textures(files, &names);
        let text: String = map
            .iter()
            .map(|(shader, look)| {
                let sides = if look.two_sided { 2 } else { 1 };
                let anim = look.anim.as_ref().map_or(String::new(), |anim| {
                    format!(
                        "\t{}\t{}\t{}",
                        anim.fps,
                        u8::from(anim.once),
                        anim.frames.join("|")
                    )
                });
                let clamp = u8::from(look.clamp);
                format!("{shader}\t{}\t{sides}\t{clamp}{anim}\n", look.texture)
            })
            .collect();
        let _ = store.put(kind, &key, text.as_bytes());
        map
    })
}

fn read_shader_textures(files: &Files, names: &[&str]) -> BTreeMap<String, ShaderLook> {
    let mut out = BTreeMap::new();
    for name in names {
        let Some(bytes) = files.read(name) else {
            continue;
        };
        let Ok(parsed) = fmt_ja::shader::parse(&bytes) else {
            continue;
        };
        for shader in parsed.shaders {
            let image = shader.stages.iter().flatten().find_map(|line| {
                let keyword = line.first()?.to_ascii_lowercase();
                match keyword.as_str() {
                    "map" | "clampmap" => {
                        let path = line.get(1).filter(|path| !path.starts_with('$'))?;
                        Some((fmt_ja::game_path(path), keyword == "clampmap", None))
                    }
                    "animmap" | "clampanimmap" | "oneshotanimmap" => {
                        let fps = line.get(1)?.parse::<f32>().ok()?;
                        let frames: Vec<String> = line
                            .get(2..)?
                            .iter()
                            .take(MAX_ANIM_FRAMES)
                            .map(|path| fmt_ja::game_path(path))
                            .collect();
                        let anim = AnimMap {
                            fps,
                            once: keyword == "oneshotanimmap",
                            frames,
                        };
                        let clamp = keyword == "clampanimmap";
                        Some((anim.frames.first()?.clone(), clamp, Some(anim)))
                    }
                    _ => None,
                }
            });
            let two_sided = shader.directives.iter().any(|line| {
                line.first()
                    .is_some_and(|word| word.eq_ignore_ascii_case("cull"))
                    && line.get(1).is_some_and(|how| {
                        [
                            "none",
                            "disable",
                            "twosided",
                            "back",
                            "backside",
                            "backsided",
                        ]
                        .iter()
                        .any(|word| how.eq_ignore_ascii_case(word))
                    })
            });
            if let Some((texture, clamp, anim)) = image {
                out.entry(shader.name).or_insert(ShaderLook {
                    texture,
                    two_sided,
                    clamp,
                    anim,
                });
            }
        }
    }
    out
}

/// The `blendFunc` of the first stage that has one, for each named shader
/// (lower-case), as its two factors (lower-case); `add` and `blend` spelt
/// out as the factors they stand for.
pub(crate) fn blend_funcs(files: &Files, names: &[String]) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let shader_files: Vec<&str> = files
        .all
        .keys()
        .copied()
        .filter(|name| name.starts_with("shaders/") && name.ends_with(".shader"))
        .collect();
    for file in shader_files {
        let Some(bytes) = files.read(file) else {
            continue;
        };
        let Ok(parsed) = fmt_ja::shader::parse(&bytes) else {
            continue;
        };
        for shader in parsed.shaders {
            if !names.contains(&shader.name) || out.contains_key(&shader.name) {
                continue;
            }
            let func = shader.stages.iter().flatten().find_map(|line| {
                let keyword = line.first()?.to_ascii_lowercase();
                if keyword != "blendfunc" {
                    return None;
                }
                let words: Vec<String> = line[1..].iter().map(|w| w.to_ascii_lowercase()).collect();
                match words.as_slice() {
                    [one] if one == "add" => Some(("gl_one".into(), "gl_one".into())),
                    [one] if one == "blend" => {
                        Some(("gl_src_alpha".into(), "gl_one_minus_src_alpha".into()))
                    }
                    [one] if one == "filter" => Some(("gl_dst_color".into(), "gl_zero".into())),
                    [src, dst, ..] => Some((src.clone(), dst.clone())),
                    _ => None,
                }
            });
            if let Some(func) = func {
                out.insert(shader.name, func);
            }
        }
    }
    out
}

/// How each effect shader (lower-case) blends: additive or by alpha, from
/// its first stage's `blendFunc`; anything else is drawn additive, with a
/// note.
pub(crate) fn effect_blends(
    files: &Files,
    names: Vec<String>,
    notes: &mut Vec<String>,
) -> BTreeMap<String, EffectBlend> {
    let funcs = blend_funcs(files, &names);
    names
        .into_iter()
        .map(|name| {
            let blend = match funcs.get(&name).map(|(s, d)| (s.as_str(), d.as_str())) {
                Some(("gl_one", "gl_one")) => EffectBlend::Add,
                Some(("gl_src_alpha", "gl_one_minus_src_alpha")) => EffectBlend::Alpha,
                Some(("gl_dst_color", "gl_src_color")) => EffectBlend::Modulate,
                other => {
                    notes.push(format!("{name}: blend {other:?} drawn as GL_ONE GL_ONE"));
                    EffectBlend::Add
                }
            };
            (name, blend)
        })
        .collect()
}

/// An effect file (`effects/….efx`) as templates; what it asks for that
/// is not supported, or its absence, goes to the notes.
pub(crate) fn effect_file(
    files: &Files,
    path: &str,
    notes: &mut Vec<String>,
) -> asset_ja::fx::Effect {
    let parsed = files
        .read(path)
        .ok_or(format!("{path}: not found"))
        .and_then(|bytes| fmt_ja::efx::parse(&bytes).map_err(|e| format!("{path}: {e}")));
    match parsed {
        Ok(group) => {
            let effect = asset_ja::fx::effect(&group);
            if !effect.unsupported.is_empty() {
                notes.push(format!(
                    "{path}: not supported: {}",
                    effect.unsupported.join(", ")
                ));
            }
            effect
        }
        Err(error) => {
            notes.push(error);
            asset_ja::fx::Effect::default()
        }
    }
}

/// A texture by path, trying the name as given and then `.tga`, `.jpg` and
/// `.png` in place of its extension (the engine's image loader looks for
/// the name without extension). TGA has no signature to guess it by, so
/// it is decoded by its extension.
fn texture(files: &Files, path: &str) -> Option<TextureIr> {
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    let (found, bytes) = [path.to_string()]
        .into_iter()
        .chain(["tga", "jpg", "png"].map(|ext| format!("{stem}.{ext}")))
        .find_map(|candidate| files.read(&candidate).map(|bytes| (candidate, bytes)))?;
    let decoded = if found.to_ascii_lowercase().ends_with(".tga") {
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga).ok()?
    } else {
        image::load_from_memory(&bytes).ok()?
    };
    let alpha = decoded.color().has_alpha();
    let rgba = decoded.to_rgba8();
    Some(TextureIr {
        width: rgba.width(),
        height: rgba.height(),
        encoding: Encoding::Rgba8,
        mips: vec![rgba.into_raw()],
        cube: false,
        frames: 1,
        facts: TextureFacts {
            alpha,
            ..TextureFacts::default()
        },
        faces: Vec::new(),
        frame_mips: Vec::new(),
    })
}

/// An animated map's frames decoded and stacked top to bottom, each at the
/// first's size (`FrameStrip`); a frame that does not load is left out.
fn frame_strip(files: &Files, anim: &AnimMap) -> Option<TextureIr> {
    let frames: Vec<TextureIr> = anim
        .frames
        .iter()
        .filter_map(|frame| texture(files, frame))
        .collect();
    let first = frames.first()?;
    let (width, height) = (first.width, first.height);
    let mut rgba = Vec::with_capacity(frames.len() * (width * height * 4) as usize);
    for frame in &frames {
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.mips[0].clone())?;
        let sized = if (frame.width, frame.height) == (width, height) {
            image
        } else {
            image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
        };
        rgba.extend(sized.into_raw());
    }
    Some(TextureIr {
        height: height * frames.len() as u32,
        mips: vec![rgba],
        facts: TextureFacts {
            alpha: frames.iter().any(|frame| frame.facts.alpha),
            strip: Some(FrameStrip {
                frames: frames.len() as u32,
                fps: anim.fps,
                once: anim.once,
            }),
            ..TextureFacts::default()
        },
        ..first.clone()
    })
}

/// Shader or texture names decoded through the shaders' first stages.
pub(crate) fn textures<'n>(
    files: &Files,
    names: impl IntoIterator<Item = &'n String>,
) -> (BTreeMap<String, TextureIr>, Vec<String>) {
    let shaders = shader_textures(files);
    let mut textures = BTreeMap::new();
    let mut missing = Vec::new();
    for name in names {
        if textures.contains_key(name) || missing.contains(name) {
            continue;
        }
        let look = shaders.get(name);
        let file = look.map_or(name.as_str(), |look| look.texture.as_str());
        let decoded = match look.and_then(|look| look.anim.as_ref()) {
            Some(anim) => frame_strip(files, anim),
            None => texture(files, file),
        };
        match decoded {
            Some(mut decoded) => {
                decoded.facts.two_sided = look.is_some_and(|look| look.two_sided);
                decoded.facts.clamp = look.is_some_and(|look| look.clamp);
                textures.insert(name.clone(), decoded);
            }
            None => missing.push(name.clone()),
        }
    }
    (textures, missing)
}

/// A GLM, the GLA its header names, and the GLA's path.
pub fn glm_and_gla(files: &Files, path: &str) -> Result<(Glm, Gla, String), String> {
    let path = fmt_ja::game_path(path);
    let glm_bytes = files.read(&path).ok_or(format!("{path}: not found"))?;
    let glm = Glm::parse(&glm_bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let gla_path = format!("{}.gla", fmt_ja::game_path(&glm.animation));
    let gla_bytes = files
        .read(&gla_path)
        .ok_or(format!("{gla_path}: not found"))?;
    let gla = Gla::parse(&gla_bytes).map_err(|error| format!("{gla_path}: {error:?}"))?;
    Ok((glm, gla, gla_path))
}

/// The `animation.cfg` beside a GLA.
pub fn animation_cfg(files: &Files, gla_path: &str) -> Result<AnimationConfig, String> {
    let cfg_path = format!(
        "{}/animation.cfg",
        gla_path.rsplit_once('/').map_or("", |(folder, _)| folder)
    );
    files
        .read(&cfg_path)
        .map(|bytes| AnimationConfig::parse(&bytes))
        .ok_or(format!("{cfg_path}: not found"))
}

/// `animation.cfg` sequences beside a GLA, comma-separated, each optionally
/// `NAME:blend_ms`, played one after another (`asset_ja::chain`) and baked
/// every 5 ms through the Ghoul2 player.
pub(crate) fn sequences(
    files: &Files,
    gla: &Gla,
    gla_path: &str,
    wanted: &str,
) -> Result<AnimationIr, String> {
    Ok(sequences_cued(files, gla, gla_path, wanted, false)?.0)
}

/// `sequences`, with when each one starts: (ms, animation name). With
/// `quirks`, the player reproduces its reference quirks
/// (`asset_ja::g2::Player::quirks`) and deaths start unblended
/// (`asset_ja::blend_ms`).
pub(crate) fn sequences_cued(
    files: &Files,
    gla: &Gla,
    gla_path: &str,
    wanted: &str,
    quirks: bool,
) -> Result<(AnimationIr, Vec<(f32, String)>), String> {
    let cfg = animation_cfg(files, gla_path)?;
    let entries: Vec<&str> = wanted.split(',').map(str::trim).collect();
    let moves: Vec<(&fmt_ja::animation_cfg::Sequence, f32)> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let before = i
                .checked_sub(1)
                .map(|j| entries[j].split(':').next().unwrap_or(entries[j]));
            let (name, blend) = match entry.split_once(':') {
                Some((name, blend)) => (
                    name,
                    blend
                        .parse::<f32>()
                        .map_err(|_| format!("{entry}: blend is not a number"))?,
                ),
                None => (*entry, asset_ja::blend_ms(before, entry, quirks)),
            };
            let sequence = cfg
                .sequences
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .ok_or(format!("{name}: not in animation.cfg"))?;
            Ok((sequence, blend))
        })
        .collect::<Result<_, String>>()?;
    let (cues, length) =
        asset_ja::chain(gla, &moves).ok_or(format!("{wanted}: frames past the GLA"))?;
    let started = cues
        .iter()
        .map(|cue| (cue.at_ms, cue.sequence.name.clone()))
        .collect();
    let mut player = asset_ja::g2::Player::new(gla);
    player.quirks = quirks;
    Ok((
        asset_ja::g2::bake_with(player, &cues, length, 5.0, wanted),
        started,
    ))
}

pub(crate) fn skin_beside(files: &Files, model: &str) -> BTreeMap<String, String> {
    let folder = model.rsplit_once('/').map_or("", |(folder, _)| folder);
    files
        .read(&format!("{folder}/model_default.skin"))
        .map(|bytes| asset_ja::parse_skin(&bytes))
        .unwrap_or_else(BTreeMap::new)
}

pub fn load(
    discovery: &Discovery,
    path: &str,
    sequence: Option<&str>,
    saber: Option<&str>,
) -> Result<LoadedScene, String> {
    let stack = stack(discovery)?;
    let files = Files::new(&stack);
    let (glm, gla, gla_path) = glm_and_gla(&files, path)?;
    let path = fmt_ja::game_path(path);
    let mut ir = asset_ja::model(&glm, &gla, &skin_beside(&files, &path));
    let clip = match sequence {
        None => None,
        Some(wanted) => Some(Arc::new(sequences(&files, &gla, &gla_path, wanted)?)),
    };
    if let Some(clip) = &clip {
        ir.animations.push((**clip).clone());
    }

    // An animated model stands on the floor above its origin.
    let floor = if clip.is_some() {
        asset_ja::ORIGIN_HEIGHT
    } else {
        0.0
    };
    let mut models = vec![Placed {
        ir,
        offset: [0.0, 0.0, floor],
        facing: 0.0,
        is_static: false,
        attach: None,
        hook: None,
        hook_local: None,
        clip,
    }];
    let mut blades = Vec::new();
    let mut notes = Vec::new();
    if let Some(name) = saber {
        let (hilt, hilt_blades, note) = hilt(&files, name)?;
        let hand = asset_ja::bolt(&glm, "*r_hand").ok_or("*r_hand: no such tag")?;
        notes.push(format!(
            "{note}; *r_hand on bone {} (rigid {})",
            hand.bone, hand.rigid
        ));
        models.push(Placed {
            ir: hilt,
            offset: [0.0; 3],
            facing: 0.0,
            is_static: false,
            attach: None,
            hook: Some((0, hand.bone + 1)),
            hook_local: Some(hand.matrix),
            clip: None,
        });
        let model = models.len() - 1;
        blades.extend(hilt_blades.into_iter().map(|b| BladeSpec { model, ..b }));
    }
    let (textures, missing) = textures(&files, &scene_textures(&models, &blades));
    Ok(LoadedScene {
        name: path.clone(),
        models,
        textures,
        missing,
        missing_body_textures: Vec::new(),
        entry: None,
        missing_models: Vec::new(),
        rooms: Vec::new(),
        own_fx: Vec::new(),
        instances: 0,
        creatures: 0,
        creature_refusals: Vec::new(),
        missing_clips: Vec::new(),
        creature_notes: notes,
        weapons: 0,
        ambient: [0.2; 3],
        fog: None,
        cameras: Vec::new(),
        map: None,
        cutscene_models: BTreeMap::new(),
        lit: false,
        blades,
        shown: Vec::new(),
        effects: crate::viewer::Effects::default(),
        doors: Vec::new(),
        instance_models: BTreeMap::new(),
        creature_views: Vec::new(),
        arms: BTreeMap::new(),
        ..LoadedScene::default()
    })
}

/// Every texture the models' meshes and the blades name.
pub(crate) fn scene_textures<'m>(
    models: impl IntoIterator<Item = &'m Placed>,
    blades: &[BladeSpec],
) -> Vec<String> {
    models
        .into_iter()
        .flat_map(|placed| {
            placed
                .ir
                .meshes
                .iter()
                .filter_map(|mesh| mesh.texture.clone())
        })
        .chain(
            blades
                .iter()
                .flat_map(|b| [b.glow.clone(), b.core.clone(), b.trail_texture.clone()]),
        )
        .collect()
}

/// A saber's `.sab` block (searched in every `ext_data/sabers/*.sab`) and
/// its hilt's GLM.
pub fn saber_and_hilt(files: &Files, name: &str) -> Result<(asset_ja::Saber, Glm), String> {
    let saber = files
        .all
        .keys()
        .filter(|path| path.starts_with("ext_data/sabers/") && path.ends_with(".sab"))
        .find_map(|path| asset_ja::saber(&files.read(path)?, name))
        .ok_or(format!("saber {name}: no .sab block"))?;
    let hilt_bytes = files
        .read(&saber.model)
        .ok_or(format!("{}: not found", saber.model))?;
    let hilt = Glm::parse(&hilt_bytes).map_err(|error| format!("{}: {error:?}", saber.model))?;
    Ok((saber, hilt))
}

/// Where a blade leaves its hilt and which way it runs, from its
/// `*bladeN` bolt: along the bolt's negative X. The axis is single-player's
/// (negative X, noting the GLM exporter turned the tag); multiplayer reads
/// negative Y of a bolt its query has turned a quarter, the same axis (QUIRKS.md Q-0002).
pub(crate) fn blade_axis(bolt: &asset_ja::Bolt) -> ([f32; 3], [f32; 3]) {
    let m = bolt.matrix;
    ([m[0][3], m[1][3], m[2][3]], [-m[0][0], -m[1][0], -m[2][0]])
}

/// A saber from its `.sab` block: the hilt model and its blade from the
/// hilt's `*blade1` bolt (`blade_axis`), blue (the engine's default
/// colour). A saber with more blades has a `*bladeN` bolt for each; one
/// whose tag is missing is left out (and said so in the note). The blades'
/// `model` is left 0 for the caller to set.
pub(crate) fn hilt(files: &Files, name: &str) -> Result<(ModelIr, Vec<BladeSpec>, String), String> {
    let (saber, hilt) = saber_and_hilt(files, name)?;
    let hilt_gla = if hilt.animation.eq_ignore_ascii_case("*default") {
        asset_ja::default_gla(&hilt.name)
    } else {
        let path = format!("{}.gla", hilt.animation.to_ascii_lowercase());
        Gla::parse(&files.read(&path).ok_or(format!("{path}: not found"))?)
            .map_err(|error| format!("{path}: {error:?}"))?
    };
    let mut blades = Vec::new();
    let mut untagged = Vec::new();
    for (i, &length) in saber.lengths.iter().enumerate() {
        let tag = format!("*blade{}", i + 1);
        let Some(bolt) = asset_ja::bolt(&hilt, &tag) else {
            // No tag, no blade: the hilt is still held.
            untagged.push(tag);
            continue;
        };
        let (origin, dir) = blade_axis(&bolt);
        blades.push(BladeSpec::new(
            origin,
            dir,
            length,
            saber.radius,
            SaberColor::Blue,
        ));
    }
    let mut note = format!(
        "saber {}: hilt {}, blades {:?}, radius {}, blade axes {:?}",
        saber.name,
        saber.model,
        saber.lengths,
        saber.radius,
        blades.iter().map(|b| b.dir).collect::<Vec<_>>()
    );
    if !untagged.is_empty() {
        note.push_str(&format!(
            ", no blade for the missing {}",
            untagged.join(", ")
        ));
    }
    Ok((
        asset_ja::model(&hilt, &hilt_gla, &skin_beside(files, &saber.model)),
        blades,
        note,
    ))
}
