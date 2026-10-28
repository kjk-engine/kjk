//! A module's script library: the compiled scripts and conversations its
//! objects and the creature templates can reach, gathered by walking what
//! names what (`gather`), and kept in the store once gathered
//! (`docs/IMPORT.md`, slice 3), so a plan reads one product instead of
//! walking the install and parsing every DLG.
//!
//! A gathering is keyed by the install's world data, the module, what the
//! walk starts from, and this build's gathering code (`GATHERER`, a hash of
//! this file and the DLG, GFF and NCS readers stamped at build time): changed
//! code never reads an old gathering.

use fmt_odyssey::ResType;
use std::collections::{BTreeMap, BTreeSet};

use asset_odyssey::module::Source;
use fmt_odyssey::dlg::Dlg;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use sim::pack::Gathered;

use crate::{Bakes, Request, imported, sheets};

/// A module's library, the strings its scripts hold, and whether its
/// gathering came from the store.
pub(crate) struct Library {
    pub library: sim::script::Library,
    pub strings: Vec<String>,
    pub stored: bool,
}

/// The hash of the gathering's code, from `build.rs`.
const GATHERER: &str = env!("KJK_LIBRARY_GATHERER");

/// Where the walk starts: the scripts named by hooks and talents, the
/// conversations objects speak, and the creature templates' conversations
/// (template, DLG).
struct Seeds<'a> {
    scripts: Vec<String>,
    talks: Vec<String>,
    spoken: &'a BTreeMap<String, String>,
}

impl Seeds<'_> {
    /// Everything the walk starts from, in order, for the key.
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let spoken = self
            .spoken
            .iter()
            .flat_map(|(template, dlg)| [template, dlg]);
        let lists: [Vec<&String>; 3] = [
            self.scripts.iter().collect(),
            self.talks.iter().collect(),
            spoken.collect(),
        ];
        for list in lists {
            out.extend((list.len() as u32).to_le_bytes());
            for name in list {
                out.extend(name.as_bytes());
                out.push(0);
            }
        }
        out
    }
}

/// The module's compiled scripts and conversations: the scripts its hooks
/// name (the module's, the area's, its objects') and those `hooked` (the
/// creature templates' hooks), its traps' rows' scripts, the DLGs its objects speak and those of the
/// creature templates (`spoken`: template, DLG) that are companions or that
/// a script names, every script a DLG names, and every script or DLG a
/// script names in a string constant (`ExecuteScript`'s,
/// `ActionStartConversation`'s), with the game's routines; and every
/// resref-like string the scripts hold. Its gathering taken from `bakes` when gathered
/// before (and kept there when not). `None` when the install has no
/// `nwscript.nss`.
pub(crate) fn script_library(
    source: &dyn Source,
    data: &sim::module::ModuleData,
    hooked: Vec<String>,
    spoken: &BTreeMap<String, String>,
    voiced: Option<&BTreeMap<String, u32>>,
    bakes: Option<(&Bakes, &Request)>,
) -> Option<Library> {
    let text: String = source
        .read("nwscript", ResType::NSS)?
        .iter()
        .map(|&byte| char::from(byte))
        .collect();
    let catalog = script_ncs::Catalog::parse(&text).ok()?;
    let talents = sheets::talents(source);
    let scripts = talents
        .impacts
        .iter()
        .filter(|name| !name.is_empty())
        .cloned()
        .chain(
            data.module_scripts
                .iter()
                .chain(&data.area_scripts)
                .map(|(_, script)| script.to_ascii_lowercase()),
        )
        .chain(data.objects().flat_map(|object| {
            let scripts = object.scripts.iter();
            scripts
                .map(|(_, script)| script.to_ascii_lowercase())
                .collect::<Vec<_>>()
        }))
        .chain(hooked)
        // The engine's own: what it runs as an in-game screen opens.
        .chain(sim::script::ENGINE_SCRIPTS.iter().map(|name| name.to_string()))
        // The traps' (`traps.2da`'s `trapscript`): a sprung trap without an
        // `OnTrapTriggered` of its own runs its row's.
        .chain(
            data.traps
                .iter()
                .filter(|row| !row.script.is_empty())
                .map(|row| row.script.to_ascii_lowercase()),
        )
        .collect();
    let talks = data
        .objects()
        .map(|object| object.conversation.clone())
        // The companions' (KOTOR names their templates `p_…`): the party is
        // talked to wherever it follows.
        .chain(
            spoken
                .iter()
                .filter(|(template, _)| template.starts_with("p_"))
                .map(|(_, dialog)| dialog.clone()),
        )
        .filter(|name| !name.is_empty())
        .collect();
    let seeds = Seeds {
        scripts,
        talks,
        spoken,
    };
    let place = bakes.map(|(bakes, request)| {
        let key = store::Key::of(
            &format!("library/{}/{GATHERER}", sim::pack::GATHERED_VERSION),
            &[
                &bakes.world,
                request.game.as_bytes(),
                request.module.as_bytes(),
                &seeds.bytes(),
            ],
        );
        let kind = format!(
            "bake/{}/library-v{}",
            request.game,
            sim::pack::GATHERED_VERSION
        );
        (bakes, kind, key)
    });
    // Deflated: a tenth of the size (about 0.3 MB a module), a millisecond
    // or two to read back.
    let baked = place.as_ref().and_then(|(bakes, kind, key)| {
        let bytes = miniz_oxide::inflate::decompress_to_vec(&bakes.store.get(kind, key)?).ok()?;
        Gathered::decode(&bytes).ok()
    });
    let stored = baked.is_some();
    let gathered = match baked {
        Some(gathered) => gathered,
        None => {
            let gathered = gather(source, seeds);
            if let Some((bakes, kind, key)) = &place {
                let bytes = miniz_oxide::deflate::compress_to_vec(&gathered.encode(), 1);
                let _ = bakes.store.put(kind, key, &bytes);
            }
            gathered
        }
    };
    let file_ms = |name: &str| {
        source
            .voice(name)
            .and_then(|file| imported::voice_ms(&file))
    };
    let mut voices = BTreeMap::new();
    for dlg in gathered.dialogues.values() {
        for node in dlg.entries.iter().chain(&dlg.replies) {
            for name in [&node.sound, &node.vo] {
                if name.is_empty() || voices.contains_key(name) {
                    continue;
                }
                // The import's lengths when there are; else the file's.
                let length = match voiced {
                    Some(voiced) => voiced.get(&name.to_ascii_lowercase()).copied(),
                    None => file_ms(name),
                };
                if let Some(ms) = length {
                    voices.insert(name.clone(), ms);
                }
            }
        }
    }
    // The alien speech TSL's entries play, its variants' files' own
    // lengths (the import measures the lines' voices only).
    let aliens = alien_voices(source);
    for dlg in gathered.dialogues.values() {
        for node in &dlg.entries {
            let Some(sound) = sim::script::alien_sound(&aliens, dlg, node) else {
                continue;
            };
            for variant in 1..=3 {
                let name = format!("{sound}{variant}");
                if !voices.contains_key(&name)
                    && let Some(ms) = file_ms(&name)
                {
                    voices.insert(name, ms);
                }
            }
        }
    }
    let clips = camera_clips(source, &gathered.dialogues);
    let gestures = dialog_animations(source, &catalog);
    // TSL's scripts play `animations.2da`'s rows by number
    // (`Library::script_animation`); K1's do not.
    let tsl = sim::combat::passive::Game::named(&data.game) == sim::combat::passive::Game::Tsl;
    let rows = match tsl {
        true => asset_odyssey::creature::animation_rows(source),
        false => Vec::new(),
    };
    let library = sim::script::Library::new(catalog, gathered.scripts, talents)
        .with_dialogues(gathered.dialogues, voices)
        .with_camera_clips(clips)
        .with_clips(stored_body_clips(
            source,
            &rows,
            bakes.map(|(bakes, request)| (bakes, request.game.as_str())),
        ))
        .with_dialog_animations(gestures)
        .with_discharges(discharges(source))
        .with_rounds(rounds(source))
        .with_animation_rows(played_rows(&rows))
        .with_alien_voices(aliens)
        .with_head_arcs(head_arcs(source))
        .with_move_rates(move_rates(source))
        .with_regeneration(regeneration(source))
        .with_health_regeneration(health_regeneration(source))
        .with_influence(influence(source))
        .with_presentation(presentation(source));
    Some(Library {
        library,
        strings: gathered.strings,
        stored,
    })
}

/// The `dialoganimations.2da` names no `ANIMATION_…` constant is named
/// for, and the constant each plays: the one whose name ends with the
/// row's (`Scratch_Head` `PAUSE_SCRATCH_HEAD`, `animations.2da`'s
/// `Idle_Scratch_Head`; `Drunk` `PAUSE_DRUNK`), the first of the numbered
/// ones (`Victory` `VICTORY1`, `Victory_Taunt`), the looping one a row
/// ending `_LP` asks for (`Use_Computer_LP`, `Use_Computer_Loop`), and the
/// choke for `Choked` (`Choke`).
/// TSL's own rows (`Nod_No`, `Shrug`, `Point`, …) play the engine clips
/// `asset_odyssey::creature::DIALOG_CLIPS` names.
const DIALOG_ALIASES: &[(&str, &str)] = &[
    ("VICTORY", "ANIMATION_FIREFORGET_VICTORY1"),
    ("SCRATCH_HEAD", "ANIMATION_FIREFORGET_PAUSE_SCRATCH_HEAD"),
    ("DRUNK", "ANIMATION_LOOPING_PAUSE_DRUNK"),
    ("USE_COMPUTER_LP", "ANIMATION_LOOPING_USE_COMPUTER"),
    ("CHOKED", "ANIMATION_LOOPING_CHOKE"),
];

/// Each `dialoganimations.2da` row's animation: the script constant named
/// for its `name` (`Talk_Forceful` is `ANIMATION_LOOPING_TALK_FORCEFUL`,
/// `Bow` `ANIMATION_FIREFORGET_BOW`), else the one `DIALOG_ALIASES` gives
/// it, else TSL's engine clip for the row (`creature::DIALOG_CLIPS`); none
/// where the row is empty or none of them names an animation.
fn dialog_animations(source: &dyn Source, catalog: &script_ncs::Catalog) -> Vec<Option<u16>> {
    let Some(table) = source
        .read("dialoganimations", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    let Some(column) = table.column("name") else {
        return Vec::new();
    };
    (0..table.cells.len())
        .map(|row| {
            let name = std::str::from_utf8(table.get(row, column)?).ok()?.trim();
            if name.is_empty() || name == "****" {
                return None;
            }
            let name = name.to_ascii_uppercase();
            let alias = DIALOG_ALIASES
                .iter()
                .find(|(row, _)| *row == name)
                .map(|(_, constant)| constant.to_string());
            ["ANIMATION_LOOPING_", "ANIMATION_FIREFORGET_"]
                .into_iter()
                .map(|kind| format!("{kind}{name}"))
                .chain(alias)
                .find_map(|constant| match catalog.constant_named(&constant)?.value {
                    script_ncs::catalog::Value::Int(value) => u16::try_from(value).ok(),
                    _ => None,
                })
                .or_else(|| {
                    let clips = asset_odyssey::creature::DIALOG_CLIPS.iter();
                    let mut own = clips.filter(|(row, ..)| row.eq_ignore_ascii_case(&name));
                    own.next().map(|&(_, code, _)| code)
                })
        })
        .collect()
}

/// TSL's alien speech: for each `alienvo.2da` row (a race), the lowercase
/// sound it says for each `emotion.2da` row, the cell under the emotion's
/// `label` (`Comment_Generic_Short` is `AVO_TwilMCmntS` for a Twi'lek
/// male); empty where the race has none for it. None in K1, which has
/// neither table.
fn alien_voices(source: &dyn Source) -> Vec<Vec<String>> {
    let table = |name: &str| {
        let bytes = source.read(name, ResType::TWO_DA)?;
        fmt_odyssey::twoda::TwoDa::parse(&bytes).ok()
    };
    let (Some(races), Some(emotions)) = (table("alienvo"), table("emotion")) else {
        return Vec::new();
    };
    let labels: Vec<String> = (0..emotions.cells.len())
        .map(|row| emotions.text(row, "label").unwrap_or_else(String::new))
        .map(|label| label.to_ascii_lowercase())
        .collect();
    (0..races.cells.len())
        .map(|race| {
            let sound = |label: &String| races.resref(race, label).unwrap_or_else(String::new);
            labels.iter().map(sound).collect()
        })
        .collect()
}

/// `weapondischarge.2da`: when (ms into the clip) each bolt of a gun's
/// attack clip leaves the gun, by the `appearance.2da` row a row is a body's
/// own for (its label's `<appearance>_` prefix: a droid's, HK-47's;
/// `ANY_BODY` without one) and the clip's `animations.2da` row (the label's
/// number), its `shots` first `shot<n>` columns and the hand each leaves
/// (`switchmask`).
fn discharges(source: &dyn Source) -> BTreeMap<(u16, u16), sim::script::effects::Discharge> {
    use sim::script::effects::Discharge;
    let Some(table) = source
        .read("weapondischarge", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (row, label) in table.row_labels.iter().enumerate() {
        let label = String::from_utf8_lossy(label);
        let (body, animation) = match label.split_once('_') {
            Some((body, animation)) => (body.parse().ok(), animation),
            None => (Some(sim::script::ANY_BODY), &*label),
        };
        let (Some(body), Ok(animation)) = (body, animation.parse::<u16>()) else {
            continue;
        };
        let shots = table.int(row, "shots").unwrap_or(0).clamp(0, 12) as usize;
        let times: Vec<u16> = (1..=shots)
            .filter_map(|shot| table.int(row, &format!("shot{shot}")))
            .filter_map(|ms| u16::try_from(ms).ok())
            .collect();
        let left = table
            .text(row, "switchmask")
            .map_or(0, |cell| Discharge::mask(&cell));
        let shots = Discharge::of(&times, left);
        out.insert((body, animation), shots);
    }
    out
}

/// What a round between creatures reads (`sim::combat::rounds::Table`):
/// the `animations.2da` rows of the attack clips (`<set><class>a<n>`) by
/// name, and `combatanimations.2da` by its rows' labels (the attack's
/// row): `hit1`, and the `parry<c>`, `dodge<c>` and `damage<c>` cells by
/// the target's class (`NONE` where empty).
fn rounds(source: &dyn Source) -> sim::combat::rounds::Table {
    use sim::combat::rounds::{Answers, CLASSES, NONE, Table};
    let read = |name| {
        source
            .read(name, ResType::TWO_DA)
            .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    };
    let (Some(animations), Some(combat)) = (read("animations"), read("combatanimations")) else {
        return Table::default();
    };
    // A name the table repeats keeps its first row, as a lookup by name
    // finds it.
    let mut rows = std::collections::BTreeMap::new();
    let named = (0..animations.cells.len()).filter_map(|row| {
        let name = animations.resref(row, "name")?.to_ascii_lowercase();
        let attack = matches!(
            name.as_bytes(),
            &[
                b'c' | b'g' | b'm' | b'f' | b'b',
                b'0'..=b'9',
                b'a',
                b'0'..=b'9'
            ]
        );
        attack.then_some((name, u16::try_from(row).ok()?))
    });
    for (name, row) in named {
        rows.entry(name).or_insert(row);
    }
    let cell = |row: usize, column: &str| {
        combat
            .int(row, column)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(NONE)
    };
    let answers = combat
        .row_labels
        .iter()
        .enumerate()
        .filter_map(|(row, label)| {
            let attack = String::from_utf8_lossy(label).parse::<u16>().ok()?;
            let mut cells = [[NONE; 3]; CLASSES];
            for (class, cells) in cells.iter_mut().enumerate() {
                *cells =
                    ["parry", "dodge", "damage"].map(|kind| cell(row, &format!("{kind}{class}")));
            }
            let hit_ms = cell(row, "hit1");
            Some((attack, Answers { hit_ms, cells }))
        })
        .collect();
    Table { rows, answers }
}

/// Each `appearance.2da` row's `head_arc_h` (degrees, 0 where blank): a
/// player's head arc, by its persona's appearance (`Library::head_arc`).
fn head_arcs(source: &dyn Source) -> Vec<f32> {
    let Some(table) = source
        .read("appearance", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    (0..table.cells.len())
        .map(|row| table.float(row, "head_arc_h").unwrap_or(0.0))
        .collect()
}

/// `creaturespeed.2da`'s `PLAYER` and `DEFAULT` rows, by `2daname`: their
/// `walkrate` and `runrate` (m/s), a player's body's and a creature's whose
/// own row reads as nothing (`sim::walk::rates`).
pub(crate) fn move_rates(source: &dyn Source) -> sim::script::MoveRates {
    let Some(table) = source
        .read("creaturespeed", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    else {
        return sim::script::MoveRates::default();
    };
    let row = |name: &str| {
        let row = (0..table.cells.len()).find(|&row| {
            table
                .text(row, "2daname")
                .is_some_and(|label| label.eq_ignore_ascii_case(name))
        })?;
        Some((table.float(row, "walkrate")?, table.float(row, "runrate")?))
    };
    sim::script::MoveRates {
        player: row("PLAYER"),
        default: row("DEFAULT"),
    }
}

/// TSL's `regeneration.2da`: its rows by label, each one `value`; none
/// when the table has no `timeperhp` row (K1's has columns per combat
/// state instead: `health_regeneration`).
fn regeneration(source: &dyn Source) -> Option<sim::script::Regeneration> {
    let table = source
        .read("regeneration", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())?;
    let value = |label: &str| {
        let row = table
            .row_labels
            .iter()
            .position(|row| row.eq_ignore_ascii_case(label.as_bytes()))?;
        table.float(row, "value")
    };
    Some(sim::script::Regeneration {
        time_per_hp: value("timeperhp")?,
        out_of_combat: value("outofcombathpbase").unwrap_or(0.0),
        in_combat: value("incombathpbase").unwrap_or(0.0),
        con_bonus: value("conmodbonus").unwrap_or(0.0),
        skill_bonus: value("skillrankbonus").unwrap_or(0.0),
        fp: sim::script::FpRegeneration {
            time: value("fpregentime").unwrap_or(0.0),
            out_of_combat: value("outofcombatfpbase").unwrap_or(0.0),
            in_combat: value("incombatfpbase").unwrap_or(0.0),
            wis_bonus: value("wismodbonus").unwrap_or(0.0),
        },
    })
}

/// K1's `regeneration.2da`: `healthregen` in row 0 (in combat) and row 1
/// (out of it), by number as its executable reads them; none for TSL's
/// table, which has no such column.
fn health_regeneration(source: &dyn Source) -> Option<sim::script::HealthRegeneration> {
    let table = source
        .read("regeneration", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())?;
    table.column("healthregen")?;
    let percent = |row: usize| table.float(row, "healthregen").unwrap_or(0.0);
    Some(sim::script::HealthRegeneration {
        in_combat: percent(0),
        out_of_combat: percent(1),
    })
}

/// TSL's `influence.2da`: each row's lowercase `tag` and `baseinfluence`;
/// none in K1, which has no such table.
fn influence(source: &dyn Source) -> Vec<(String, u8)> {
    let Some(table) = source
        .read("influence", ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    (0..table.cells.len())
        .filter_map(|row| {
            let tag = table.text(row, "tag")?.trim().to_ascii_lowercase();
            let base = u8::try_from(table.int(row, "baseinfluence")?).ok()?;
            (!tag.is_empty()).then_some((tag, base))
        })
        .collect()
}

/// What the presentation routines read (`sim::script::Presentation`):
/// `tutorial.2da`'s rows with `message_pc0` set, `feedbacktext.2da`'s
/// `strref`s, `appearance.2da`'s `normalhead`s; empty where a table is
/// missing.
fn presentation(source: &dyn Source) -> sim::script::Presentation {
    let table = |name: &str| {
        source
            .read(name, ResType::TWO_DA)
            .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
    };
    let column = |name: &str, column: &str| -> Vec<Option<i64>> {
        table(name).map_or(Vec::new(), |table| {
            (0..table.cells.len())
                .map(|row| table.int(row, column))
                .collect()
        })
    };
    // TSL's PC words; K1's table has one column of them.
    let (pc, plain) = (
        column("tutorial", "message_pc0"),
        column("tutorial", "message0"),
    );
    let words = |row: usize| {
        pc.get(row)
            .copied()
            .flatten()
            .or(plain.get(row).copied().flatten())
    };
    sim::script::Presentation {
        tutorials: (0..pc.len().max(plain.len()))
            .map(|row| words(row).is_some_and(|strref| strref != 0))
            .collect(),
        feedback: (column("feedbacktext", "strref").iter())
            .map(|strref| {
                strref
                    .and_then(|strref| u32::try_from(strref).ok())
                    .unwrap_or(0)
            })
            .collect(),
        heads: (column("appearance", "normalhead").iter())
            .map(|head| head.and_then(|head| i16::try_from(head).ok()).unwrap_or(-1))
            .collect(),
    }
}

/// The clips of the conversations' camera models and their lengths (ms),
/// by lowercase model and clip name: what a line that waits on its clip
/// lasts.
fn camera_clips(
    source: &dyn Source,
    dialogues: &BTreeMap<String, Dlg>,
) -> BTreeMap<(String, String), u32> {
    let models: BTreeSet<String> = dialogues
        .values()
        .map(|dlg| dlg.camera_model.to_ascii_lowercase())
        .filter(|model| !model.is_empty())
        .collect();
    let mut clips = BTreeMap::new();
    for model in models {
        let Some(mdl) = source.read(&model, ResType::MDL) else {
            continue;
        };
        let Ok((_, timings)) = fmt_odyssey::mdl::Model::timings(&mdl) else {
            continue;
        };
        for timing in timings {
            let ms = (timing.length.max(0.0) * 1000.0).round() as u32;
            clips.insert((model.clone(), timing.name.to_ascii_lowercase()), ms);
        }
    }
    clips
}

/// A model's supermodel (lowercase), and its clips' names (lowercase, as
/// the views' clips have them) and lengths (seconds).
#[derive(Clone)]
struct Timings {
    supermodel: Option<String>,
    clips: Vec<(String, f32)>,
}

/// What scripts and conversations play on a body by number, and the clip
/// names: the constants and the conversations' own clips
/// (`creature::scripted_clips`), and `rows`' (`creature::row_clips`: TSL's
/// rows played by number; none for K1).
fn played_clips(rows: &[(String, bool)]) -> Vec<(u16, &str)> {
    let mut clips: Vec<(u16, &str)> = asset_odyssey::creature::scripted_clips().collect();
    clips.extend(asset_odyssey::creature::row_clips(rows));
    clips
}

/// Each of `rows` as the animation it plays (`creature::row_animation`;
/// `u16::MAX` for a row with no name): `Library::with_animation_rows`.
fn played_rows(rows: &[(String, bool)]) -> Vec<u16> {
    (rows.iter().zip(0u16..))
        .map(|((name, looping), row)| match name.is_empty() {
            true => u16::MAX,
            false => asset_odyssey::creature::row_animation(row, *looping),
        })
        .collect()
}

/// The sim numbers looping rows as the views do.
const _: () = assert!(sim::combat::idle::LOOPED_ROWS == asset_odyssey::creature::LOOPED);

/// `body_clips`, kept in `bakes` once made for the install's world data
/// (`game`'s), under this file's code (`GATHERER`) and the clip table.
fn stored_body_clips(
    source: &dyn Source,
    rows: &[(String, bool)],
    bakes: Option<(&Bakes, &str)>,
) -> BTreeMap<(u16, u16), u32> {
    let table: Vec<u8> = played_clips(rows)
        .into_iter()
        .flat_map(|(animation, name)| {
            [&animation.to_le_bytes()[..], name.as_bytes(), &[0]].concat()
        })
        .collect();
    let place = bakes.map(|(bakes, game)| {
        let key = store::Key::of(&format!("clips/{GATHERER}"), &[&bakes.world, &table]);
        (bakes, format!("bake/{game}/clips-v1"), key)
    });
    // Each entry 8 bytes: the row, the constant, the length.
    let decode = |bytes: &[u8]| -> Option<BTreeMap<(u16, u16), u32>> {
        let (entries, rest) = bytes.as_chunks::<8>();
        let entry = |&[r0, r1, a0, a1, m0, m1, m2, m3]: &[u8; 8]| {
            let (row, animation) = (u16::from_le_bytes([r0, r1]), u16::from_le_bytes([a0, a1]));
            ((row, animation), u32::from_le_bytes([m0, m1, m2, m3]))
        };
        rest.is_empty().then(|| entries.iter().map(entry).collect())
    };
    if let Some(clips) = place
        .as_ref()
        .and_then(|(bakes, kind, key)| decode(&bakes.store.get(kind, key)?))
    {
        return clips;
    }
    let clips = body_clips(source, rows);
    if let Some((bakes, kind, key)) = &place {
        let bytes: Vec<u8> = clips
            .iter()
            .flat_map(|(&(row, animation), ms)| {
                [row.to_le_bytes(), animation.to_le_bytes()]
                    .concat()
                    .into_iter()
                    .chain(ms.to_le_bytes())
            })
            .collect();
        let _ = bakes.store.put(kind, key, &bytes);
    }
    clips
}

/// How long the clip each animation of `played_clips` names plays once on
/// the body
/// of each `appearance.2da` row (ms), by row and constant, where its model
/// has it: the row's unarmoured model (`creature::resolve_appearance`; the
/// armour's models share its supermodel chain [verify]), its clips looked
/// up as the views look them up (`launcher::viewer::scripted_clips`): by
/// lowercase name, a model's own `c…` name first through its whole chain
/// for a creature model type (`creature::creature_model`), then the shared
/// one, its own clips before its supermodel's. Each model's timings are
/// read once, from its header (`Model::timings`).
fn body_clips(source: &dyn Source, animation_rows: &[(String, bool)]) -> BTreeMap<(u16, u16), u32> {
    use asset_odyssey::creature;
    let mut tables = creature::Tables::default();
    let rows = tables
        .get(source, "appearance")
        .map_or(0, |table| table.cells.len());
    // Each model's timings, read once.
    let mut read: BTreeMap<String, Option<Timings>> = BTreeMap::new();
    let mut models: BTreeMap<(String, String), Vec<(u16, u32)>> = BTreeMap::new();
    let mut clips = BTreeMap::new();
    for row in 0..u16::try_from(rows).unwrap_or(u16::MAX) {
        let Ok(look) = creature::resolve_appearance(source, u32::from(row), None, &mut tables)
        else {
            continue;
        };
        let key = (look.body.to_ascii_lowercase(), look.model_type);
        let lengths = models.entry(key.clone()).or_insert_with(|| {
            // The model and its supermodels, nearest first (a loop stops
            // at eight).
            let mut chain: Vec<Timings> = Vec::new();
            let mut next = Some(key.0.clone());
            for _ in 0..8 {
                let Some(name) = next.take() else {
                    break;
                };
                let timings = read.entry(name.clone()).or_insert_with(|| {
                    let mdl = source.read(&name, ResType::MDL)?;
                    let (supermodel, own) = fmt_odyssey::mdl::Model::timings(&mdl).ok()?;
                    Some(Timings {
                        supermodel: supermodel.map(|name| name.to_ascii_lowercase()),
                        clips: own
                            .into_iter()
                            .map(|t| (t.name.to_ascii_lowercase(), t.length))
                            .collect(),
                    })
                });
                let Some(timings) = timings.clone() else {
                    break;
                };
                next = timings.supermodel.clone();
                chain.push(timings);
            }
            let own_names = creature::creature_model(&key.1);
            let length = |name: &str| {
                chain
                    .iter()
                    .flat_map(|timings| &timings.clips)
                    .find(|(clip, _)| clip == name)
                    .map(|clip| clip.1)
            };
            played_clips(animation_rows)
                .into_iter()
                .filter_map(|(animation, name)| {
                    let seconds = own_names
                        .then(|| length(&format!("c{name}")))
                        .flatten()
                        .or_else(|| length(name))?;
                    Some((animation, (seconds.max(0.0) * 1000.0).round() as u32))
                })
                .collect()
        });
        clips.extend(
            lengths
                .iter()
                .map(|&(animation, ms)| ((row, animation), ms)),
        );
    }
    clips
}

/// The walk: each conversation read once, each script once, until nothing
/// new is named.
fn gather(source: &dyn Source, seeds: Seeds) -> Gathered {
    let (ncs, dlg) = (ResType::NCS, ResType::DLG);
    let Seeds {
        scripts: mut waiting,
        mut talks,
        spoken,
    } = seeds;
    let mut scripts = BTreeMap::new();
    let mut dialogues = BTreeMap::new();
    // Conversations looked for, read or not (one that does not is left out).
    let mut tried = BTreeSet::new();
    let mut strings = Vec::new();
    loop {
        if let Some(name) = talks.pop() {
            if !tried.insert(name.clone()) {
                continue;
            }
            let Some(bytes) = source.read(&name, dlg) else {
                continue;
            };
            if let Ok(parsed) = Dlg::parse(&bytes) {
                waiting.extend(parsed.scripts().into_iter().map(str::to_string));
                dialogues.insert(name, parsed);
            }
            continue;
        }
        let Some(name) = waiting.pop() else {
            break;
        };
        if scripts.contains_key(&name) {
            continue;
        }
        let Some(program) = source
            .read(&name, ncs)
            .and_then(|bytes| Ncs::parse(&bytes).ok())
        else {
            continue;
        };
        for instruction in &program.code {
            if let Op::Const(Const::String(bytes)) = &instruction.op {
                let named = String::from_utf8_lossy(bytes).to_ascii_lowercase();
                let resref = !named.is_empty()
                    && named.len() <= 16
                    && named
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if !resref {
                    continue;
                }
                if !tried.contains(&named) && source.exists(&named, dlg) {
                    talks.push(named.clone());
                }
                // A creature template a script names (`CreateObject`'s): its
                // conversation too.
                if let Some(dialog) = spoken.get(&named) {
                    talks.push(dialog.clone());
                }
                if !scripts.contains_key(&named) && source.exists(&named, ncs) {
                    waiting.push(named.clone());
                }
                strings.push(named);
            }
        }
        scripts.insert(name, program);
    }
    strings.sort();
    strings.dedup();
    Gathered {
        scripts,
        dialogues,
        strings,
    }
}

/// The names of the clips `model` holds itself (not its supermodel's),
/// lowercase; none where `source` has no such model.
pub fn clip_names(source: &dyn Source, model: &str) -> Vec<String> {
    source
        .read(model, ResType::MDL)
        .and_then(|mdl| fmt_odyssey::mdl::Model::timings(&mdl).ok())
        .map_or_else(Vec::new, |(_, own)| {
            own.into_iter()
                .map(|t| t.name.to_ascii_lowercase())
                .collect()
        })
}

/// Each `dialoganimations.2da` row's animation on `source`'s data, its
/// constants read from its own `nwscript.nss` (`dialog_animations`).
fn gesture_rows(source: &dyn Source) -> Vec<Option<u16>> {
    let text: Option<String> = source
        .read("nwscript", ResType::NSS)
        .map(|bytes| bytes.iter().map(|&byte| char::from(byte)).collect());
    text.and_then(|text| script_ncs::Catalog::parse(&text).ok())
        .map_or_else(Vec::new, |catalog| dialog_animations(source, &catalog))
}

/// The scripts' clip names and the conversations' gestures on the games'
/// data (`k1`, `tsl`; each skipped without its install): on K1's,
/// `GET_LOW`, `GET_MID` and `PAUSE_DRUNK` name clips of the humanoid
/// supermodel (`s_male02`), and the `dialoganimations.2da` rows named for
/// no constant play their aliases' (`DIALOG_ALIASES`), `Ready` `LOOPING_READY`,
/// the empty row 1 none; on TSL's, every named row plays something, each
/// of `creature::DIALOG_CLIPS` its row's, numbered by the `animations.2da`
/// row of its clip's name and looping as the dialog row does, the commonest
/// (`Nod_No`, `Nod_Yes`, `Shrug`, `Point_Loop`, `Talk_Prone_Loop`) in TSL's
/// humanoid supermodels; and TSL's alien speech (`tsl_alien_voices`).
pub fn clip_probes(
    k1: Option<&dyn Source>,
    tsl: Option<&dyn Source>,
) -> Vec<(&'static str, Result<String, String>)> {
    let mut checks = match k1 {
        Some(source) => k1_clip_probes(source),
        None => vec![("clip names", Ok("skipped: no K1 install".into()))],
    };
    for (name, probe) in [
        ("tsl dialog gestures", tsl_gestures as fn(&dyn Source) -> _),
        ("tsl alien voices", tsl_alien_voices),
        ("tsl rows by number", tsl_rows_by_number),
        ("tsl script clip rows", |source| {
            script_clip_rows(source, true)
        }),
    ] {
        checks.push((
            name,
            tsl.map_or_else(|| Ok("skipped: no TSL install".into()), probe),
        ));
    }
    checks
}

/// A probe's result: `what` passed or failed.
fn checked(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

/// `clip_probes` on K1's data.
fn k1_clip_probes(source: &dyn Source) -> Vec<(&'static str, Result<String, String>)> {
    let clips = clip_names(source, "s_male02");
    let named = |animation: u16| {
        asset_odyssey::creature::SCRIPTED_CLIPS
            .iter()
            .find(|(a, ..)| *a == animation)
            .map(|(.., name)| *name)
    };
    let missing: Vec<u16> = [10, 11, 13]
        .into_iter()
        .filter(|&a| named(a).is_none_or(|name| !clips.iter().any(|clip| clip == name)))
        .collect();
    let rows = gesture_rows(source);
    let wanted = [
        (1, None),
        (44, Some(108)),
        (55, Some(102)),
        (58, Some(13)),
        (121, Some(15)),
        (148, Some(19)),
        (150, Some(116)),
    ];
    let wrong: Vec<_> = wanted
        .iter()
        .filter(|&&(row, want)| rows.get(row).copied().flatten() != want)
        .map(|&(row, _)| (row, rows.get(row).copied().flatten()))
        .collect();
    vec![
        (
            "clip names",
            checked(
                missing.is_empty() && !clips.is_empty(),
                format!(
                    "GET_LOW {:?}, GET_MID {:?}, PAUSE_DRUNK {:?} among s_male02's {} clips (missing: {missing:?})",
                    named(10),
                    named(11),
                    named(13),
                    clips.len()
                ),
            ),
        ),
        (
            "dialog gestures",
            checked(
                wrong.is_empty() && !rows.is_empty(),
                format!(
                    "rows 44 Victory, 55 Scratch_Head, 58 Drunk, 121 Use_Computer_LP, 150 Choked play their aliases' constants, 148 Ready 19, the empty row 1 none (wrong: {wrong:?})"
                ),
            ),
        ),
        ("script clip rows", script_clip_rows(source, false)),
    ]
}

/// Every `creature::SCRIPTED_CLIPS` clip is the `name` of the
/// `animations.2da` row it gives, in this game's table: all of them in
/// TSL's (`every`), in K1's those of K1's constants (to 32, and 100 to 120;
/// TSL's own rows start at 394).
fn script_clip_rows(source: &dyn Source, every: bool) -> Result<String, String> {
    let rows = asset_odyssey::creature::animation_rows(source);
    let k1 = |constant: u16| constant <= 32 || (100..=120).contains(&constant);
    let wrong: Vec<u16> = asset_odyssey::creature::SCRIPTED_CLIPS
        .iter()
        .filter(|&&(constant, row, clip)| match rows.get(usize::from(row)) {
            Some((name, _)) => name != clip,
            None => every || k1(constant),
        })
        .map(|&(constant, ..)| constant)
        .collect();
    checked(
        wrong.is_empty() && !rows.is_empty(),
        format!(
            "{} constants' clips are their animations.2da rows' names, of {} rows (wrong: {wrong:?})",
            asset_odyssey::creature::SCRIPTED_CLIPS.len(),
            rows.len()
        ),
    )
}

/// `clip_probes` on TSL's data.
fn tsl_gestures(source: &dyn Source) -> Result<String, String> {
    use asset_odyssey::creature::{DIALOG_CLIPS, LOOPED};
    let table = |name: &str| {
        let bytes = source.read(name, ResType::TWO_DA)?;
        fmt_odyssey::twoda::TwoDa::parse(&bytes).ok()
    };
    let (Some(dialog), Some(animations)) = (table("dialoganimations"), table("animations")) else {
        return Err("no dialoganimations.2da or animations.2da".into());
    };
    let rows = gesture_rows(source);
    let named = |row: usize| dialog.text(row, "name").filter(|name| !name.is_empty());
    let silent: Vec<usize> = (0..dialog.cells.len())
        .filter(|&row| named(row).is_some() && rows.get(row).copied().flatten().is_none())
        .collect();
    let wrong: Vec<&str> = DIALOG_CLIPS
        .iter()
        .filter(|&&(name, code, clip)| {
            let row = (0..dialog.cells.len()).find(|&row| named(row).as_deref() == Some(name));
            let looping = code >= LOOPED;
            let animation = usize::from(code - if looping { LOOPED } else { 10_000 });
            let right = row.is_some_and(|row| {
                rows.get(row).copied().flatten() == Some(code)
                    && dialog.flag(row, "looping") == looping
            }) && animations
                .text(animation, "name")
                .is_some_and(|own| own.eq_ignore_ascii_case(clip));
            !right
        })
        .map(|&(name, ..)| name)
        .collect();
    // The rest are the clips of one character's own model (Kreia's
    // `lhandchop`, Bao-Dur's `powerpunch`).
    let shared: Vec<String> = ["s_male02", "s_female02", "s_female03"]
        .into_iter()
        .flat_map(|model| clip_names(source, model))
        .collect();
    let held = |clip: &str| shared.iter().any(|own| own == clip);
    let missing: Vec<&str> = ["nodno", "nodyes", "shrug", "pointloop", "talkprone"]
        .into_iter()
        .filter(|clip| !held(clip))
        .collect();
    checked(
        silent.is_empty() && wrong.is_empty() && missing.is_empty() && !rows.is_empty(),
        format!(
            "{} rows, the named ones playing nothing {silent:?}; {} engine clips, wrong \
             {wrong:?}, {} in the humanoid supermodels (missing of the commonest: {missing:?})",
            rows.len(),
            DIALOG_CLIPS.len(),
            DIALOG_CLIPS.iter().filter(|(.., clip)| held(clip)).count(),
        ),
    )
}

/// TSL's scripts' animations by `animations.2da` row on its data
/// (`Library::script_animation`): 10 000 and a row plays the row, looping
/// as its `looping` says (Kreia's 10316 `animloop01` and 10074 `horror`
/// loop, 10040 `getfromgnd` once), and a humanoid times it
/// (`played_clips`); 10 000 itself, a negative value and K1's 10 000 and
/// up play the pause loop, a row past the table nothing.
fn tsl_rows_by_number(source: &dyn Source) -> Result<String, String> {
    use asset_odyssey::creature::{LOOPED, ONCE};
    let text: String = source
        .read("nwscript", ResType::NSS)
        .ok_or("no nwscript.nss")?
        .iter()
        .map(|&byte| char::from(byte))
        .collect();
    let catalog = script_ncs::Catalog::parse(&text).map_err(|e| format!("{e:?}"))?;
    let rows = asset_odyssey::creature::animation_rows(source);
    let library = sim::script::Library::new(catalog, BTreeMap::new(), sheets::talents(source))
        .with_animation_rows(played_rows(&rows));
    let cases = [
        (10_316, true, LOOPED + 316),
        (10_074, true, LOOPED + 74),
        (10_040, true, ONCE + 40),
        (10_000, true, 0),
        (-1, true, 0),
        (19_999, true, u16::MAX),
        (10_316, false, 0),
        (5, true, 5),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .map(|&(value, tsl, want)| (value, tsl, library.script_animation(value, tsl), want))
        .filter(|(.., got, want)| got != want)
        .collect();
    let named = |row: usize| rows.get(row).map_or("", |(name, _)| name.as_str());
    let clips = clip_names(source, "s_male02");
    let timed: Vec<&str> = [40, 74]
        .into_iter()
        .map(named)
        .filter(|name| clips.iter().any(|clip| clip == name))
        .collect();
    let played = played_clips(&rows);
    let listed = [(LOOPED + 74, named(74)), (ONCE + 40, named(40))]
        .iter()
        .all(|wanted| played.contains(wanted));
    checked(
        wrong.is_empty() && timed.len() == 2 && listed && named(316) == "animloop01",
        format!(
            "{} rows; row 316 {:?}, 74 {:?}, 40 {:?}; s_male02 times {timed:?}; wrong (value, tsl, got, want): {wrong:?}",
            rows.len(),
            rows.get(316),
            rows.get(74),
            rows.get(40)
        ),
    )
}

/// TSL's alien speech on its data (`alien_voices`): a Twi'lek male's
/// (`alienvo.2da` row 24) scared line (`emotion.2da` row 22,
/// `Scared_Medium`) is `avo_twilmscarm`, its three variants installed. Not
/// every sound the table names is (K1's Rakata, Selkath and Wookiees are
/// named and not shipped): those count, as lines that play nothing.
fn tsl_alien_voices(source: &dyn Source) -> Result<String, String> {
    let sounds = alien_voices(source);
    let installed =
        |sound: &str| (1..=3).all(|variant| source.voice(&format!("{sound}{variant}")).is_some());
    let scared = sounds.get(24).and_then(|row| row.get(22)).cloned();
    let named: BTreeSet<&String> = sounds.iter().flatten().filter(|s| !s.is_empty()).collect();
    let shipped = named.iter().filter(|sound| installed(sound)).count();
    checked(
        scared.as_deref() == Some("avo_twilmscarm") && installed("avo_twilmscarm"),
        format!(
            "{} races, {} sounds, {shipped} with all three variants installed; Twi'lek male \
             scared {scared:?}",
            sounds.len(),
            named.len()
        ),
    )
}
