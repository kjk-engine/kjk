//! What a creature looks like: body model, body texture and head, from its
//! UTC template, its equipped armour and `appearance.2da`.
//!
//! The rule, where the references read the data alike (and what each does
//! where they do not):
//!
//! - Model type `B` (character): body `model<v>`, texture `tex<v>` plus the
//!   armour's texture variation as two digits, falling back to `01` when that
//!   texture does not exist. `v` is the body variation letter: `a` without
//!   armour. With armour it is `baseitems.2da` `bodyvar` for the item's
//!   base item, one capital letter (A to N in the games' data; `a` for any
//!   other cell, see `body_letter`). KOTOR's
//!   executable sets an item's body variation from its base item when it
//!   loads the item and never reads the UTI's `BodyVariation` field. Mods
//!   fill that field with anything (Solomon's Revenge's robes carry 50 to
//!   64), and reading it drew them in underwear. `bodyvar_disagrees`
//!   records a UTI whose field names another letter. When the row names
//!   no model for that letter, the `a` columns are used (both references
//!   draw nothing; Q-0137).
//! - Other types: body `race`, texture `racetex`.
//! - Head, for `B` (and `P`, in xoreos): `heads.2da` `head` of `normalhead`;
//!   xoreos falls back to `backuphead`, reone does not.
//! - The body texture replaces the diffuse of every body mesh that names one,
//!   when the install has it: reone sets it only once it loads, so a body
//!   whose `racetex` the game does not ship keeps its model's own textures;
//!   xoreos puts it on anyway and the body is not drawn (QUIRKS Q-0132).
//! - Env map, as KOTOR's executable sets it when an appearance is applied:
//!   `envmap` names a cube map for the body and the head (not the weapons
//!   held); `DEFAULT`, or an empty cell, takes the area's ARE
//!   `DefaultEnvMap`, and an empty one of those sets none (`EnvMap`).

use std::collections::{BTreeMap, BTreeSet};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;

pub use crate::tables::Tables;
use fmt_odyssey::twoda::TwoDa;

use crate::module::Source;

/// The head and body slots' bits in `Equip_ItemList` struct ids: they are
/// inventory slots 0 and 1, and a struct's id is `1 << slot`.
const HEAD_SLOT_ID: u32 = 1 << 0;
const BODY_SLOT_ID: u32 = 1 << 1;
/// Right and left weapon slots (inventory slots 4 and 5).
const RIGHT_WEAPON_ID: u32 = 1 << 4;
const LEFT_WEAPON_ID: u32 = 1 << 5;

#[derive(Debug, Clone, PartialEq)]
pub struct CreatureLook {
    pub appearance: u32,
    pub model_type: String,
    pub body: String,
    pub body_texture: Option<String>,
    /// The body texture `appearance.2da` names when the install lacks it
    /// (`body_texture` is then `None`).
    pub missing_body_texture: Option<String>,
    pub head: Option<String>,
    /// The armour, when one is equipped in the body slot.
    pub armour: Option<String>,
    /// (the UTI's letter, the base item's `bodyvar`) when they differ.
    pub bodyvar_disagrees: Option<(char, String)>,
    /// The head came from `backuphead` (xoreos only).
    pub backup_head: bool,
    /// The model of what it wears on its head (`item_model`), hung on its
    /// head model's `gogglehook` (as reone hangs a mask).
    pub headgear: Option<String>,
    /// Weapon models for `rhand` and `lhand` (`weapon_model`).
    pub right_weapon: Option<String>,
    pub left_weapon: Option<String>,
    /// Its sound set: its `soundset.2da` row (the UTC's `SoundSetFile`).
    pub sound_set: Option<u16>,
    /// The sound its sound set's death line plays (`sound_set_line`).
    pub death_sound: Option<String>,
    /// The env map its body and head are drawn with (`envmap`).
    pub envmap: EnvMap,
    /// The `heads.2da` row of its head, when it has one.
    pub head_row: Option<usize>,
    /// The texture every textured mesh of its head wears instead of the
    /// head model's own (`align`), when one does.
    pub head_texture: Option<String>,
    /// Its body's variation letter (`a` without armour) and whether it
    /// wears armour, for its dark side body texture (`align`).
    pub letter: char,
    pub armoured: bool,
    /// Its template's alignment (`GoodEvil`), when it came from one.
    pub good_evil: Option<u8>,
}

/// How the executables dress a creature for its alignment
/// (`CSWCCreatureAppearance`'s alignment pass), as `align` applies it.
/// The game decides the numbers (`dark_step`); the textures are the
/// install's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Aligned {
    /// How far to the dark side its head and portrait have gone: 0 not at
    /// all, then 1 to 4, each a darker texture (`HEAD_COLUMNS`,
    /// `PORTRAIT_COLUMNS`).
    pub step: u8,
    /// Whether its body wears its dark side texture (`tex<letter>evil`).
    pub evil_body: bool,
    /// KOTOR 2: a head with no dark texture for the step wears its
    /// `heads.2da` `alttexture`, when it names one, as KOTOR 2 builds every
    /// head.
    pub alt_texture: bool,
}

/// The `heads.2da` column of a head's texture at each dark step (1 to 4).
pub const HEAD_COLUMNS: [&str; 4] = ["headtexe", "headtexve", "headtexvve", "headtexvvve"];

/// The `portraits.2da` column of a portrait at each dark step (0 to 4).
pub const PORTRAIT_COLUMNS: [&str; 5] = [
    "baseresref",
    "baseresrefe",
    "baseresrefve",
    "baseresrefvve",
    "baseresrefvvve",
];

/// The dark step of `good_evil` (0 dark to 100 light): how many of
/// `steps` (the most alignment of each step, lightest first) it is at or
/// below.
pub fn dark_step(good_evil: u8, steps: [u8; 4]) -> u8 {
    steps.iter().filter(|most| good_evil <= **most).count() as u8
}

/// `portraits.2da` row `row`'s portrait at dark step `step`: that step's
/// column, else the row's own (`baseresref`), as the executables fall back
/// when a column is empty.
pub fn portrait_at(portraits: &TwoDa, row: usize, step: u8) -> Option<String> {
    PORTRAIT_COLUMNS
        .get(usize::from(step))
        .filter(|_| step > 0)
        .and_then(|column| portraits.resref(row, column))
        .or_else(|| portraits.resref(row, PORTRAIT_COLUMNS[0]))
}

/// The `appearance.2da` column of a body's dark side texture: `texaevil`
/// without armour; with armour, KOTOR 2's by the body's letter (`b`, `i`
/// and `l` their own, `n` reads `texmevil`), none for any other letter and
/// none in KOTOR 1, whose table has only `texaevil`.
fn evil_column(letter: char, armoured: bool) -> Option<&'static str> {
    match (armoured, letter) {
        (false, _) => Some("texaevil"),
        (true, 'b') => Some("texbevil"),
        (true, 'i') => Some("texievil"),
        (true, 'l') => Some("texlevil"),
        (true, 'n') => Some("texmevil"),
        _ => None,
    }
}

/// `look` dressed for its alignment as the executables dress a creature
/// with a head: its head's texture at `aligned.step` (`HEAD_COLUMNS`),
/// else, with `alt_texture`, its `alttexture`; its body's dark side
/// texture with variation `01` when `evil_body`. A texture the install
/// lacks is not worn.
pub fn align(source: &dyn Source, look: &mut CreatureLook, aligned: Aligned, tables: &mut Tables) {
    let (Some(row), Some(heads)) = (look.head_row, tables.get(source, "heads")) else {
        return;
    };
    let exists = |name: &String| texture_exists(source, name);
    let dark = HEAD_COLUMNS
        .get(usize::from(aligned.step).wrapping_sub(1))
        .and_then(|column| heads.resref(row, column))
        .filter(exists);
    look.head_texture = dark.or_else(|| {
        aligned
            .alt_texture
            .then(|| heads.resref(row, "alttexture"))
            .flatten()
            .filter(exists)
    });
    if !aligned.evil_body || look.model_type != "B" {
        return;
    }
    let evil = evil_column(look.letter, look.armoured)
        .and_then(|column| {
            tables
                .get(source, "appearance")?
                .resref(look.appearance as usize, column)
        })
        .map(|base| format!("{base}01"))
        .filter(exists);
    if let Some(evil) = evil {
        look.body_texture = Some(evil);
        look.missing_body_texture = None;
    }
}

/// An object's env map as `appearance.2da` `envmap` gives it: a cube map
/// by name, or the area's (`DEFAULT`, or an empty cell, as the executable
/// reads the column).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EnvMap {
    Named(String),
    #[default]
    AreaDefault,
}

impl EnvMap {
    /// The env map an `envmap` cell names.
    pub fn of_cell(cell: Option<String>) -> EnvMap {
        match cell {
            Some(name) if !name.eq_ignore_ascii_case("default") => EnvMap::Named(name),
            _ => EnvMap::AreaDefault,
        }
    }

    /// The cube map it draws with in an area whose `DefaultEnvMap` is
    /// `area`: none where that is empty.
    pub fn resolve(&self, area: Option<&str>) -> Option<String> {
        match self {
            EnvMap::Named(name) => Some(name.clone()),
            EnvMap::AreaDefault => area.filter(|name| !name.is_empty()).map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatureRefusal {
    NoTemplate(String),
    BadTemplate(String),
    NoAppearanceType,
    NoAppearanceTable,
    NoAppearanceRow(u32),
    NoBodyModel(u32),
}

fn texture_exists(source: &dyn Source, name: &str) -> bool {
    source.exists(name, ResType::TPC) || source.exists(name, ResType::TGA)
}

/// `name`'s file of `kind` in `source`: an item of a module's own by its
/// resref (`fmt_odyssey::resref_of`).
fn gff(source: &dyn Source, name: &str, kind: ResType) -> Option<Gff> {
    Gff::parse(&source.read(fmt_odyssey::resref_of(name), kind)?).ok()
}

pub fn resolve(source: &dyn Source, template: &str) -> Result<CreatureLook, CreatureRefusal> {
    resolve_with(source, template, &mut Tables::default())
}

/// The armour (an item template) a creature template (`utc`) starts with
/// in its body slot: what it wears until its items change.
pub fn template_armour(utc: &Gff) -> Option<String> {
    utc.root
        .list("Equip_ItemList")
        .iter()
        .find(|item| item.id == BODY_SLOT_ID)
        .and_then(|item| item.resref("EquippedRes"))
}

/// `resolve`, its tables from (and kept in) `tables`.
pub fn resolve_with(
    source: &dyn Source,
    template: &str,
    tables: &mut Tables,
) -> Result<CreatureLook, CreatureRefusal> {
    let bytes = source
        .read(template, ResType::UTC)
        .ok_or_else(|| CreatureRefusal::NoTemplate(template.to_string()))?;
    let utc =
        Gff::parse(&bytes).map_err(|error| CreatureRefusal::BadTemplate(error.to_string()))?;
    let appearance = utc
        .root
        .int("Appearance_Type")
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(CreatureRefusal::NoAppearanceType)?;
    let mut look = resolve_appearance(source, appearance, template_armour(&utc), tables)?;
    look.good_evil = utc
        .root
        .int("GoodEvil")
        .and_then(|value| u8::try_from(value).ok());
    let mut worn = |slot: u32| {
        let name = utc
            .root
            .list("Equip_ItemList")
            .iter()
            .find(|item| item.id == slot)
            .and_then(|item| item.resref("EquippedRes"))?;
        item_model_with(source, &name, tables)
    };
    look.headgear = worn(HEAD_SLOT_ID);
    look.right_weapon = worn(RIGHT_WEAPON_ID);
    look.left_weapon = worn(LEFT_WEAPON_ID);
    look.sound_set = utc
        .root
        .int("SoundSetFile")
        .and_then(|row| u16::try_from(row).ok());
    look.death_sound = look
        .sound_set
        .and_then(|row| sound_set_line(source, row, fmt_odyssey::ssf::slots::DEAD, tables));
    Ok(look)
}

/// The sound sound set `row` (of `soundset.2da`) plays for its line `slot`
/// (`fmt_odyssey::ssf::slots`): the SSF's strref there, the talk table's
/// sound for it, said in the set's own voice (`own_line`).
pub fn sound_set_line(
    source: &dyn Source,
    row: u16,
    slot: usize,
    tables: &mut Tables,
) -> Option<String> {
    let sets = tables.get(source, "soundset")?;
    let name = sets.resref(usize::from(row), "resref")?;
    let set = fmt_odyssey::ssf::Ssf::parse(&source.read(&name, ResType::SSF)?).ok()?;
    let talk = source.talk()?;
    let sound = |strref: &u32| {
        let entry = talk.entries.get(*strref as usize)?;
        (!entry.sound.is_empty()).then(|| entry.sound.to_ascii_lowercase())
    };
    let line = sound(set.strrefs.get(slot)?)?;
    let names: BTreeSet<String> = (0..sets.cells.len())
        .filter_map(|row| sets.resref(row, "resref"))
        .collect();
    let lines: BTreeSet<String> = set.strrefs.iter().filter_map(sound).collect();
    let recorded = || {
        talk.entries
            .iter()
            .filter(|entry| !entry.sound.is_empty())
            .map(|entry| entry.sound.to_ascii_lowercase())
    };
    Some(own_line(&name, &lines, &names, line, recorded))
}

/// The voice a sound line is in: its sound's name up to its last `_`
/// (`c_drdsntry` for `c_drdsntry_hit1`), and the line's kind after it
/// (`hit1`). A conversation's recording (`nm44aabrok99004_`, which K1's
/// commoners say as grunts) ends in its `_` and is in no voice.
fn voice_of(sound: &str) -> Option<(&str, &str)> {
    sound.rsplit_once('_').filter(|(_, kind)| !kind.is_empty())
}

/// The voice of the sound set filed as `set` (its `soundset.2da` resref),
/// whose lines play the sounds `lines`. Its voice is the one most of its
/// lines are in, leaving out a voice named as another set's file
/// (`names`): a line in that voice is that set's (`c_bantha_hit1` is the
/// Bantha set's). A set with no line left has the voice of the
/// recordings (`recorded`: every sound the talk table names) whose name
/// spells the set's file name's letters in order, skipping some, the
/// longest such (`c_drdasasin` for `c_drdassassin`). K1's assassin droid
/// set says only other sets' lines (a brith's battle cry, an astromech's
/// attack grunt, a bantha's pain grunt and death), so it has this voice.
fn set_voice<I: Iterator<Item = String>>(
    set: &str,
    lines: &BTreeSet<String>,
    names: &BTreeSet<String>,
    recorded: impl Fn() -> I,
) -> Option<String> {
    let own = |voice: &str| voice == set || !names.contains(voice);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (voice, _) in lines.iter().filter_map(|line| voice_of(line)) {
        if own(voice) {
            *counts.entry(voice).or_default() += 1;
        }
    }
    let most = counts.values().copied().max();
    if let Some(voice) = counts
        .iter()
        .find(|(_, count)| Some(**count) == most)
        .map(|(voice, _)| voice.to_string())
    {
        return Some(voice);
    }
    let kind = set.find('_').map_or(0, |at| at + 1);
    recorded()
        .filter_map(|sound| voice_of(&sound).map(|(voice, _)| voice.to_string()))
        .filter(|voice| voice.len() > kind && own(voice) && spelled_in(voice, set))
        .max_by(|one, other| one.len().cmp(&other.len()).then_with(|| other.cmp(one)))
}

/// Whether `short`'s letters are `long`'s in order, some of `long`'s
/// skipped, starting with `long`'s first.
fn spelled_in(short: &str, long: &str) -> bool {
    let mut rest = long.chars();
    short.chars().next() == long.chars().next()
        && short
            .chars()
            .all(|letter| rest.any(|other| other == letter))
}

/// The sound a sound set (`set`, `lines`, `names` and `recorded` as
/// `set_voice` reads them) plays for a line whose sound is `line`: the
/// line itself when it is in the set's voice, else the recording of the
/// same kind in that voice (`c_drdasasin_hit1` for `c_bantha_hit1`). A
/// line whose voice has no such recording, or a set whose voice is not
/// found, plays the line as the SSF says. BioWare's own data says some
/// lines in another set's voice: KOTOR's executables play the SSF's line
/// as it is (a divergence the owner confirmed on 2026-11-12, Q-0362).
pub fn own_line<I: Iterator<Item = String>>(
    set: &str,
    lines: &BTreeSet<String>,
    names: &BTreeSet<String>,
    line: String,
    recorded: impl Fn() -> I,
) -> String {
    let Some((voice, kind)) = voice_of(&line) else {
        return line;
    };
    let Some(own) = set_voice(set, lines, names, &recorded) else {
        return line;
    };
    if voice == own {
        return line;
    }
    let wanted = format!("{own}_{kind}");
    if recorded().any(|sound| sound == wanted) {
        wanted
    } else {
        line
    }
}

/// The sound sets' probes (`kjk sim probes --only "sound sets"`): the rule
/// of the set's own voice (`own_line`) on sets made up here, and on each
/// game's data where this machine has its install. K1's Vulkar sentries
/// (sound set 3, the assassin droid's) say the assassin droid's own
/// lines, and no line of any set of either game plays another set's
/// recording.
pub fn sound_set_probes(
    k1: Option<&dyn Source>,
    tsl: Option<&dyn Source>,
) -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let set_of = |sounds: &[&str]| -> BTreeSet<String> {
        sounds.iter().map(|sound| sound.to_string()).collect()
    };
    let names = set_of(&[
        "c_bantha",
        "c_brith",
        "c_drdastro",
        "c_drdassassin",
        "c_drdmkfour",
        "c_khounda",
        "c_khoundb",
    ]);
    let recorded = set_of(&[
        "c_bantha_hit1",
        "c_drdastro_atk1",
        "c_drdasasin_atk1",
        "c_drdasasin_hit1",
        "c_drdasasin_dead",
        "c_drdmk4_hit1",
        "c_drdmk4_dead",
        "c_drdmk1_dead",
        "c_khounda_hit1",
    ]);
    let said = |set: &str, lines: &[&str], line: &str| {
        own_line(set, &set_of(lines), &names, line.to_string(), || {
            recorded.iter().cloned()
        })
    };
    let assassin = [
        "c_brith_bat1",
        "c_drdastro_atk1",
        "c_bantha_hit1",
        "c_bantha_dead",
    ];
    let made_up = [
        said("c_drdassassin", &assassin, "c_bantha_hit1"),
        said("c_drdassassin", &assassin, "c_drdastro_atk1"),
        said(
            "c_drdmkfour",
            &["c_drdmk4_hit1", "c_drdmk4_atk1", "c_drdmk1_dead"],
            "c_drdmk1_dead",
        ),
        said("c_khoundb", &["c_khounda_hit1"], "c_khounda_hit1"),
        said("c_bantha", &["c_bantha_hit1"], "c_bantha_hit1"),
    ];
    let mut checks = vec![(
        "own voice",
        check(
            made_up
                == [
                    "c_drdasasin_hit1",
                    "c_drdasasin_atk1",
                    "c_drdmk4_dead",
                    "c_khounda_hit1",
                    "c_bantha_hit1",
                ],
            format!("{made_up:?}"),
        ),
    )];
    checks.push((
        "vulkar sentry",
        k1.map_or_else(
            || Ok("skipped: no K1 install".into()),
            |source| {
                let tables = &mut Tables::default();
                let lines: Vec<Option<String>> = [0, 9, 12, 15]
                    .map(|slot| sound_set_line(source, 3, slot, tables))
                    .into();
                let wanted = [
                    "c_drdasasin_bat1",
                    "c_drdasasin_atk1",
                    "c_drdasasin_hit1",
                    "c_drdasasin_dead",
                ];
                check(
                    lines
                        .iter()
                        .zip(wanted)
                        .all(|(line, wanted)| line.as_deref() == Some(wanted)),
                    format!("sound set 3 battle cry, attack, pain and death: {lines:?}"),
                )
            },
        ),
    ));
    for (name, install) in [
        ("k1 sets in own voices", k1),
        ("tsl sets in own voices", tsl),
    ] {
        checks.push((
            name,
            install.map_or_else(|| Ok("skipped: no install".into()), sets_in_own_voices),
        ));
    }
    checks
}

/// Every line of every sound set in `source`'s `soundset.2da` checked: the
/// sound it plays (`sound_set_line`) is in no other set's voice (named as
/// another set's file).
fn sets_in_own_voices(source: &dyn Source) -> Result<String, String> {
    let tables = &mut Tables::default();
    let sets = tables.get(source, "soundset").ok_or("no soundset.2da")?;
    let names: Vec<(u16, String)> = (0..sets.cells.len())
        .filter_map(|row| Some((u16::try_from(row).ok()?, sets.resref(row, "resref")?)))
        .collect();
    let (mut lines, mut borrowed) = (0, Vec::new());
    for (row, set) in &names {
        let Some(ssf) = source
            .read(set, ResType::SSF)
            .and_then(|bytes| fmt_odyssey::ssf::Ssf::parse(&bytes).ok())
        else {
            continue;
        };
        for slot in 0..ssf.strrefs.len() {
            let Some(sound) = sound_set_line(source, *row, slot, tables) else {
                continue;
            };
            lines += 1;
            let voice = voice_of(&sound).map(|(voice, _)| voice);
            if names
                .iter()
                .any(|(_, other)| other != set && Some(other.as_str()) == voice)
            {
                borrowed.push(format!("{set} line {slot} {sound}"));
            }
        }
    }
    let what = format!(
        "{lines} lines of {} sets, in another set's voice: {borrowed:?}",
        names.len()
    );
    if borrowed.is_empty() {
        Ok(what)
    } else {
        Err(what)
    }
}

/// The animations `animations.2da` marks `hideequippeditems`: a
/// character's equipped items are hidden while it plays them.
pub fn hiding_animations(source: &dyn Source) -> Vec<String> {
    let Some(table) = source
        .read("animations", ResType::TWO_DA)
        .and_then(|bytes| TwoDa::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    (0..table.cells.len())
        .filter(|row| table.int(*row, "hideequippeditems") == Some(1))
        .filter_map(|row| table.resref(row, "name"))
        .collect()
}

/// The `ANIMATION_…` constants scripts play that a body may have a clip for,
/// the `animations.2da` row each plays and the clip: that row's `name`. The
/// row is the one whose description says the constant (`TALK_NORMAL`
/// `Talk_Normal`, `PAUSE_TIRED` `Idle_Tired`: a pause is an idle; a looping
/// constant the `_Loop` row where there is one, `USE_COMPUTER`'s
/// `Use_Computer_Loop`), else whose name does (`DANCE1` `dance1`, `WELD`
/// `weld`, the kneeling talks `kdtlkangry` and `kdtlksad`), as the comments
/// below say where neither is plain. Both games' tables share their rows (TSL
/// adds rows from 394 on; `session::clip_probes` checks every name against
/// each game's table), and both number the constants alike (K1's are TSL's up
/// to 32 and 120), so one table serves both: a body without a clip of the
/// name plays none. A creature model (`modeltype` S or L) plays the name with
/// a `c` before it where its chain has that (HK-47's `ctaunt`), else this one
/// (its `tlknorm`). Where no row's name or description says the constant,
/// both executables' `PlayAnimation` (read off the Mac builds) picks one:
/// `WORSHIP` (4) meditates, `VICTORY2`/`3` (109, 110) are `VICTORY`, and
/// `CUSTOM01` (119) plays row 346's `usecomp`, the clip only the Taris
/// rancor's model holds (it eats from the bone pile so, `k_ptar_rancorcs1`);
/// `DEACTIVATE` (20) the looping `disabled`, TSL's chair sits (36 to 40)
/// the loops `animloop01` to `05`, `CHECK_BODY` (33) `disablemine` and
/// `SCREAM` (124) `forcerage`. `READY` (19) plays the body's wield's ready
/// stance, in the view (`game::acting`).
pub const SCRIPTED_CLIPS: &[(u16, u16, &str)] = &[
    (0, 6, "pause1"),
    (1, 7, "pause2"),
    (2, 18, "listen"),
    (3, 24, "meditate"),
    (4, 24, "meditate"),
    (5, 25, "tlknorm"),
    (6, 27, "tlkplead"),
    (7, 26, "tlkforce"),
    (8, 29, "tlklaugh"),
    (9, 28, "tlksad"),
    // `GET_LOW` and `GET_MID` play the pick-ups from the ground and from a
    // table; `PAUSE_DRUNK` the poisoned idle (`Idle_Poisoned`), the one
    // unsteady idle the rows hold [verify].
    (10, 40, "getfromgnd"),
    (11, 41, "getfromtbl"),
    (12, 14, "pausetrd"),
    (13, 15, "pausepsn"),
    (14, 32, "flirt"),
    (15, 44, "usecomplp"),
    (16, 53, "dance"),
    (17, 54, "dance1"),
    (18, 74, "horror"),
    // The droid's looping disabled stance (`Disabled_Droid`), not row 349's
    // one-off `deactivate`: what both executables play for it.
    (20, 270, "disabled"),
    (21, 77, "spasm"),
    (22, 76, "sleep"),
    (23, 79, "prone"),
    (24, 359, "pause3"),
    (25, 360, "weld"),
    (26, 81, "dead"),
    (27, 370, "talkinj"),
    (28, 371, "listeninj"),
    (29, 35, "treatinjlp"),
    // The dead loop added with the kneeling talks (rows 373–385) [verify].
    (30, 375, "dead3"),
    (31, 384, "kdtlkangry"),
    (32, 385, "kdtlksad"),
    // TSL's `CHECK_BODY` kneels as over a mine (`Disable_Mine_On_Ground`).
    (33, 51, "disablemine"),
    (34, 47, "unlockdr"),
    (35, 457, "meditatesit"),
    // The chair sits (`SIT_CHAIR`, its `_DRINK`, `_PAZAK`, `_COMP1`, `_COMP2`)
    // are the five loops `animloop01` to `05` (rows 316 to 320) the
    // humanoid models hold, in that order: Atton at the Ebon Hawk's helm
    // is `_COMP2`.
    (36, 316, "animloop01"),
    (37, 317, "animloop02"),
    (38, 318, "animloop03"),
    (39, 319, "animloop04"),
    (40, 320, "animloop05"),
    // A fire-and-forget row (`Perform_Force_Rage`) [verify].
    (41, 461, "forcerage"),
    (43, 333, "closed"),
    (44, 5, "stealth"),
    // TSL's looping choke: 116 is the fire-and-forget one too.
    (45, 72, "choke"),
    (46, 458, "meditatestand"),
    (100, 11, "hturnl"),
    (101, 10, "hturnr"),
    (102, 12, "pausesh"),
    (103, 13, "pausebrd"),
    (104, 16, "salute"),
    (105, 19, "bow"),
    (106, 31, "greeting"),
    (107, 33, "taunt"),
    (108, 17, "victory"),
    (109, 17, "victory"),
    (110, 17, "victory"),
    (112, 37, "inject"),
    (113, 43, "usecomp"),
    (114, 68, "persuade"),
    (115, 38, "activate"),
    (116, 72, "choke"),
    // The grenade throws (`Throw_Grenade_1`, `_2`), which high and which
    // low [verify].
    (117, 57, "throwgren"),
    (118, 58, "throwgren1"),
    (119, 346, "usecomp"),
    (120, 34, "treatinj"),
    // `Casting_Outward_1` [verify].
    (121, 62, "castout1"),
    (122, 310, "open"),
    (123, 567, "diveroll"),
    // `SCREAM` plays the Force rage.
    (124, 461, "forcerage"),
];

/// Where the engine's clips a body plays for its time, looping, are
/// numbered from: this and their `animations.2da` row. Those numbered from
/// `ONCE` and their row play once (the dying clip, a mine's disarm).
pub const LOOPED: u16 = 20_000;
pub const ONCE: u16 = 10_000;

/// An `animations.2da` row's number as a clip: `LOOPED` and the row where
/// its `looping` column says so, else `ONCE` and the row.
pub fn row_animation(row: u16, looping: bool) -> u16 {
    row + if looping { LOOPED } else { ONCE }
}

/// Each `animations.2da` row's lowercase `name` (empty where it has none)
/// and whether its `looping` column is set: both executables' clients name
/// a row's clip and loop it so (`row_animation`).
pub fn animation_rows(source: &dyn Source) -> Vec<(String, bool)> {
    let Some(table) = source
        .read("animations", ResType::TWO_DA)
        .and_then(|bytes| TwoDa::parse(&bytes).ok())
    else {
        return Vec::new();
    };
    (0..table.cells.len())
        .map(|row| {
            let name = table.resref(row, "name").unwrap_or_else(String::new);
            (
                name.to_ascii_lowercase(),
                table.int(row, "looping") == Some(1),
            )
        })
        .collect()
}

/// The clips of `rows` (`animation_rows`) by their numbers
/// (`row_animation`): what TSL's scripts play as 10 000 and a row, where
/// the row has a name.
pub fn row_clips(rows: &[(String, bool)]) -> impl Iterator<Item = (u16, &str)> {
    rows.iter()
        .zip(0u16..)
        .filter(|((name, _), _)| !name.is_empty())
        .map(|((name, looping), row)| (row_animation(row, *looping), name.as_str()))
}

/// TSL's `dialoganimations.2da` rows no `ANIMATION_…` constant is named for,
/// the engine clip each plays on a humanoid (numbered as `LOOPED` says, by
/// the row's `looping` column) and the clip's name. A line's `AnimList`
/// value is TSL's animation number (10 000 and the dialog row), which the
/// exe's client turns into an `animations.2da` row for a humanoid body
/// (`appearance.2da` creature type 1), the row's `name` the clip; a body of
/// another type finds no row for these and plays nothing. The table is
/// that mapping for the rows scripts cannot name: `Nod_No` is row 467,
/// `nodno`; `Collapse` the collapsed loop (`Collapselp`), played once as
/// its dialog row says.
pub const DIALOG_CLIPS: &[(&str, u16, &str)] = &[
    ("Touch_Heart", 10_462, "touchheart"),
    ("Roll_Eyes", 10_463, "rolleyes"),
    ("Use_Item_On_Other", 10_464, "itemequip"),
    ("Stand_Attention", LOOPED + 465, "standstill"),
    ("Nod_Yes", 10_466, "nodyes"),
    ("Nod_No", 10_467, "nodno"),
    ("Point", 10_468, "point"),
    ("Point_Loop", LOOPED + 469, "pointloop"),
    ("Point_Down", 10_470, "pointdown"),
    ("Scanning", 10_471, "scanning"),
    ("Shrug", 10_472, "shrug"),
    ("Cut_Hands", 10_557, "cuthand"),
    ("L_Hand_Chop", 10_558, "lhandchop"),
    ("Collapse", 10_560, "collapselp"),
    ("Collapse_Stand", 10_561, "collapsestand"),
    ("Power_Punch", 10_562, "powerpunch"),
    ("Point_Raise_Arm", 10_563, "uppoint"),
    ("Point_Lower_Arm", 10_564, "downpoint"),
    ("Hood_Off", 10_565, "offhood"),
    ("Hood_On", 10_566, "onhood"),
    ("Talk_Prone_Loop", LOOPED + 568, "talkprone"),
    ("Talk_Prone_Die", 10_569, "talkpronedie"),
];

/// The clips of a Force power's casting, `animations.2da` rows by name, for
/// `spells.2da`'s `castanim` (both executables read that column alone for
/// both phases; `conjanim` is never read): the row played once for the
/// power's `conjtime`, and the one looped for its `casttime` (none for a jump
/// and a monster's, which stay on the first). A word no row uses is `point`'s.
pub fn cast_rows(castanim: &str) -> (u16, Option<u16>) {
    match castanim.to_ascii_lowercase().as_str() {
        "self" => (62, Some(63)),
        "dark" => (64, Some(65)),
        "up" => (66, Some(67)),
        "throw" => (69, Some(70)),
        "jump" | "monster" | "area" => (62, None),
        _ => (62, Some(70)),
    }
}

/// `cast_rows`' clips: what a body shows a cast by, numbered as
/// `row_animation` does.
pub const CAST_CLIPS: &[(u16, &str)] = &[
    (ONCE + 62, "castout1"),
    (LOOPED + 63, "castoutlp1"),
    (ONCE + 64, "castout2"),
    (LOOPED + 65, "castoutlp2"),
    (ONCE + 66, "castout3"),
    (LOOPED + 67, "castoutlp3"),
    (ONCE + 69, "throwsab"),
    (LOOPED + 70, "throwsablp"),
];

/// What scripts and conversations play on a body that it may have a clip
/// for, and the clip's name: the constants (`SCRIPTED_CLIPS`), the
/// conversations' own clips (`DIALOG_CLIPS`) and their cutscene clips
/// (`dlg::cut_clips`). The views draw and the sim times them by this one
/// table.
pub fn scripted_clips() -> impl Iterator<Item = (u16, &'static str)> {
    static CUTS: std::sync::LazyLock<Vec<(u16, String)>> =
        std::sync::LazyLock::new(|| fmt_odyssey::dlg::cut_clips().collect());
    let dialog = DIALOG_CLIPS.iter().map(|&(_, code, clip)| (code, clip));
    let cuts = CUTS.iter().map(|(code, clip)| (*code, clip.as_str()));
    SCRIPTED_CLIPS
        .iter()
        .map(|&(constant, _, clip)| (constant, clip))
        .chain(dialog)
        .chain(cuts)
}

/// The cutscene clips are numbered as the rest are: played once below
/// `LOOPED`, past every row numbered from `ONCE`; looped from past every
/// row numbered from `LOOPED`.
const _: () = assert!(
    fmt_odyssey::dlg::CUT_ONCE >= ONCE + fmt_odyssey::dlg::FIRST_CUT
        && fmt_odyssey::dlg::CUT_ONCE + 2 * fmt_odyssey::dlg::CUT_RUN <= LOOPED
        && fmt_odyssey::dlg::CUT_LOOPED >= LOOPED + fmt_odyssey::dlg::FIRST_CUT
);

/// Whether a body of `appearance.2da` `modeltype` `model_type` is a creature
/// model (S or L), whose clips may carry a `c` before their names: KOTOR
/// asks such a body for its clips by those names (`cpause1`, `cdie`,
/// `cdead`, `cvictory`; SCRIPTS.md), whatever its
/// skeleton, so a gammorean the views retarget as a humanoid dies by its
/// own `cdie`. Where its chain lacks the creature name the shared one is
/// played (Q-0136's droids). The views and the sim's clip lengths
/// (`session::library`) both ask this.
pub fn creature_model(model_type: &str) -> bool {
    matches!(model_type.to_ascii_uppercase().as_str(), "S" | "L")
}

/// A player's sound set: the `soundset.2da` row labelled
/// `Player_<Female|Male>_W`, by its portrait's (`portraits.2da` row
/// `portrait`) sex. K1's `Player_…_B` rows have no recordings in the
/// install and are taken as unused (a K1 save with portrait `po_pmha2`
/// holds `Player_Male_W`). TSL has no such rows: its player has none.
pub fn player_sound_set(source: &dyn Source, portrait: u16, tables: &mut Tables) -> Option<u16> {
    let portraits = tables.get(source, "portraits")?;
    let row = usize::from(portrait);
    portraits.resref(row, "baseresref")?;
    let female = portraits.int(row, "sex") == Some(1);
    let label = if female {
        "player_female_w"
    } else {
        "player_male_w"
    };
    let sets = tables.get(source, "soundset")?;
    (0..sets.cells.len())
        .find(|row| sets.resref(*row, "label").as_deref() == Some(label))
        .and_then(|row| u16::try_from(row).ok())
}

/// The body variation letter a `baseitems.2da` `bodyvar` cell gives, as
/// KOTOR 2's executable reads it when it loads the base items. A cell of
/// one capital letter from A to Z gives that letter. Any other cell, a
/// small letter included, gives `a`. KOTOR 2's armour uses K to N: the
/// flight suit K, the slave outfit L, Zeison Sha armour M, and the Jedi
/// Knight and Jedi Master robes N.
///
/// KOTOR 1's executable stops at J and gives `a` for K to Z. KOTOR 1's
/// `appearance.2da` has no model column past J, so the fallback to the `a`
/// columns in `resolve_appearance` draws the same body there (QUIRKS
/// Q-0386).
fn body_letter(cell: &str) -> char {
    match cell.as_bytes() {
        [letter] if letter.is_ascii_uppercase() => char::from(letter.to_ascii_lowercase()),
        _ => 'a',
    }
}

/// The look of `appearance.2da` row `appearance` wearing `armour` (a UTI's
/// resref) in its body slot, holding nothing: a template's (`resolve_with`),
/// or one made without a template (character creation's choices).
pub fn resolve_appearance(
    source: &dyn Source,
    appearance: u32,
    armour: Option<String>,
    tables: &mut Tables,
) -> Result<CreatureLook, CreatureRefusal> {
    let appearances = tables
        .get(source, "appearance")
        .ok_or(CreatureRefusal::NoAppearanceTable)?;
    let row = appearance as usize;
    if row >= appearances.cells.len() {
        return Err(CreatureRefusal::NoAppearanceRow(appearance));
    }
    let model_type = appearances
        .resref(row, "modeltype")
        .unwrap_or_else(String::new)
        .to_ascii_uppercase();

    // The body-slot item: its UTI's variations and base item.
    let item = armour
        .as_deref()
        .and_then(|name| gff(source, name, ResType::UTI));
    let (letter, texture_variation, bodyvar_disagrees) = match &item {
        None => ('a', 1, None),
        Some(uti) => {
            let from_base_item = uti.root.int("BaseItem").and_then(|base| {
                let base_items = tables.get(source, "baseitems")?;
                base_items.text(usize::try_from(base).ok()?, "bodyvar")
            });
            let letter = from_base_item.as_deref().map_or('a', body_letter);
            let body_variation = uti.root.int("BodyVariation").unwrap_or(1);
            let from_uti = (b'a' as i64 + body_variation - 1).clamp(0, 255) as u8 as char;
            let disagrees = from_base_item
                .filter(|_| from_uti != letter)
                .map(|base_item| (from_uti, base_item));
            let variation = uti.root.int("TextureVar").unwrap_or(1);
            (letter, variation, disagrees)
        }
    };

    // A row that names no model for the armour's letter (TSL's unique rows
    // fill only `a`) draws its unarmoured body (QUIRKS Q-0137).
    let letter = if appearances.resref(row, &format!("model{letter}")).is_some() {
        letter
    } else {
        'a'
    };
    let (body, body_texture) = if model_type == "B" {
        let body = appearances.resref(row, &format!("model{letter}"));
        let texture = appearances
            .resref(row, &format!("tex{letter}"))
            .map(|base| {
                let wanted = format!("{base}{texture_variation:02}");
                if texture_exists(source, &wanted) {
                    wanted
                } else {
                    format!("{base}01")
                }
            });
        (body, texture)
    } else {
        (
            appearances.resref(row, "race"),
            appearances.resref(row, "racetex"),
        )
    };
    let body = body.ok_or(CreatureRefusal::NoBodyModel(appearance))?;
    let (body_texture, missing_body_texture) = match body_texture {
        Some(name) if !texture_exists(source, &name) => (None, Some(name)),
        found => (found, None),
    };

    let (mut head, mut backup_head) = (None, false);
    if model_type == "B" || model_type == "P" {
        let heads = tables.get(source, "heads");
        let pick = |column: &str| {
            let index = usize::try_from(appearances.int(row, column)?).ok()?;
            Some((heads.as_ref()?.resref(index, "head")?, index))
        };
        head = pick("normalhead");
        if head.is_none() {
            head = pick("backuphead");
            backup_head = head.is_some();
        }
    }
    let (head, head_row) = head.unzip();
    Ok(CreatureLook {
        head_row,
        head_texture: None,
        letter,
        armoured: item.is_some(),
        good_evil: None,
        headgear: None,
        right_weapon: None,
        left_weapon: None,
        sound_set: None,
        death_sound: None,
        envmap: EnvMap::of_cell(appearances.resref(row, "envmap")),
        appearance,
        model_type,
        body,
        body_texture,
        missing_body_texture,
        head,
        armour,
        bodyvar_disagrees,
        backup_head,
    })
}

/// The metres a walk cycle and a run cycle of `appearance.2da` row
/// `appearance` cover (`walkdist`, `rundist`).
pub fn strides(source: &dyn Source, appearance: u32, tables: &mut Tables) -> Option<[f32; 2]> {
    let appearances = tables.get(source, "appearance")?;
    let row = appearance as usize;
    let stride = |column: &str| appearances.float(row, column);
    Some([stride("walkdist")?, stride("rundist")?])
}

/// How far a body of `appearance.2da` row `appearance` leans with the
/// floor under it (`groundtilt`; 0 upright, 1 wholly with the floor; 0
/// where blank). The game reads the column as a number; its rows hold 0
/// or 1 (T3-M4, the astromechs, the Mark Two and Four droids, banthas,
/// dewbacks and rontos lean).
pub fn ground_tilt(source: &dyn Source, appearance: u32, tables: &mut Tables) -> f32 {
    tables
        .get(source, "appearance")
        .and_then(|appearances| appearances.float(appearance as usize, "groundtilt"))
        .unwrap_or(0.0)
}

/// A weapon's model (`item::model_name`).
pub fn weapon_model(source: &dyn Source, base_item: i64, variation: i64) -> Option<String> {
    let base_items = Tables::default().get(source, "baseitems")?;
    weapon_model_in(&base_items, base_item, variation)
}

/// An item template's model: its base item's, at its `ModelVariation` (a
/// lightsaber's is its blade's colour).
pub fn item_model(source: &dyn Source, template: &str) -> Option<String> {
    let base_items = Tables::default().get(source, "baseitems")?;
    item_model_in(source, &base_items, template)
}

/// `item_model`, its tables from (and kept in) `tables`.
pub fn item_model_with(source: &dyn Source, template: &str, tables: &mut Tables) -> Option<String> {
    let base_items = tables.get(source, "baseitems")?;
    item_model_in(source, &base_items, template)
}

/// How an item template is drawn lying on the floor: its model
/// (`item_model`) and its base item's `rotateonground`
/// (`item::ground_turn`), 0 where the row has none.
pub fn floor_model(source: &dyn Source, template: &str) -> Option<(String, i64)> {
    let base_items = Tables::default().get(source, "baseitems")?;
    let uti = gff(source, template, ResType::UTI)?;
    let base = uti.root.int("BaseItem")?;
    let model = weapon_model_in(
        &base_items,
        base,
        uti.root.int("ModelVariation").unwrap_or(0),
    )?;
    let turn = usize::try_from(base)
        .ok()
        .and_then(|row| base_items.int(row, "rotateonground"))
        .unwrap_or(0);
    Some((model, turn))
}

/// Whether `appearance.2da` row `row` names `body` as a body, under any of
/// the `model<letter>` columns `body_letter` can pick (KOTOR 1's table has
/// `modela` to `modelj`, KOTOR 2's `modela` to `modeln`).
pub fn names_body(appearances: &TwoDa, row: usize, body: &str) -> bool {
    ('a'..='z').any(|letter| {
        appearances
            .resref(row, &format!("model{letter}"))
            .is_some_and(|model| model.eq_ignore_ascii_case(body))
    })
}

fn item_model_in(source: &dyn Source, base_items: &TwoDa, template: &str) -> Option<String> {
    let uti = gff(source, template, ResType::UTI)?;
    weapon_model_in(
        base_items,
        uti.root.int("BaseItem")?,
        uti.root.int("ModelVariation").unwrap_or(0),
    )
}

fn weapon_model_in(base_items: &TwoDa, base_item: i64, variation: i64) -> Option<String> {
    let base = usize::try_from(base_item).ok()?;
    let class = base_items.resref(base, "itemclass")?;
    Some(crate::item::model_name(&class, variation))
}
