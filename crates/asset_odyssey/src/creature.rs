//! What a creature looks like: body model, body texture and head, from its
//! UTC template, its equipped armour and `appearance.2da`.
//!
//! The rule, where the references read the data alike (and what each does
//! where they do not):
//!
//! - Model type `B` (character): body `model<v>`, texture `tex<v>` plus the
//!   armour's texture variation as two digits, falling back to `01` when that
//!   texture does not exist. `v` is the body variation letter: `a` without
//!   armour. With armour it can be read two ways: `a + BodyVariation − 1`
//!   (the UTI field) or `baseitems.2da` `bodyvar` for the item's base item.
//!   Both are computed; `bodyvar_disagrees` records a difference and the
//!   UTI's letter is used. When the row names no model for that letter, the
//!   `a` columns are used (both references draw nothing; Q-0137).
//! - Other types: body `race`, texture `racetex`.
//! - Head, for `B` (and `P`, in xoreos): `heads.2da` `head` of `normalhead`;
//!   xoreos falls back to `backuphead`, reone does not.
//! - The body texture replaces the diffuse of every body mesh that names one,
//!   when the install has it: reone sets it only once it loads, so a body
//!   whose `racetex` the game does not ship keeps its model's own textures;
//!   xoreos puts it on anyway and the body is not drawn (QUIRKS Q-0132).

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;

pub use crate::tables::Tables;
use fmt_odyssey::twoda::TwoDa;

use crate::module::Source;

/// The body slot's bit in `Equip_ItemList` struct ids: the body is
/// inventory slot 1, and a struct's id is `1 << slot`.
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
    /// Weapon models for `rhand` and `lhand` (`weapon_model`).
    pub right_weapon: Option<String>,
    pub left_weapon: Option<String>,
    /// Its sound set: its `soundset.2da` row (the UTC's `SoundSetFile`).
    pub sound_set: Option<u16>,
    /// The sound its sound set's death line plays (`sound_set_line`).
    pub death_sound: Option<String>,
    /// Its body's chest enlarged by this much when drawn (`body_morph`).
    pub chest: Option<f32>,
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

fn gff(source: &dyn Source, name: &str, kind: ResType) -> Option<Gff> {
    Gff::parse(&source.read(name, kind)?).ok()
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
    let mut weapon = |slot: u32| {
        let name = utc
            .root
            .list("Equip_ItemList")
            .iter()
            .find(|item| item.id == slot)
            .and_then(|item| item.resref("EquippedRes"))?;
        let base_items = tables.get(source, "baseitems")?;
        item_model_in(source, &base_items, &name)
    };
    look.right_weapon = weapon(RIGHT_WEAPON_ID);
    look.left_weapon = weapon(LEFT_WEAPON_ID);
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
/// sound for it.
pub fn sound_set_line(
    source: &dyn Source,
    row: u16,
    slot: usize,
    tables: &mut Tables,
) -> Option<String> {
    let sets = tables.get(source, "soundset")?;
    let name = sets.resref(usize::from(row), "resref")?;
    let set = fmt_odyssey::ssf::Ssf::parse(&source.read(&name, ResType::SSF)?).ok()?;
    let strref = *set.strrefs.get(slot)?;
    let entry = source.talk()?.entries.get(strref as usize)?.clone();
    (!entry.sound.is_empty()).then(|| entry.sound.to_ascii_lowercase())
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
/// rancor's model holds (it eats from the bone pile so, `k_ptar_rancorcs1`).
/// TSL's `CHECK_BODY` (33), `SIT_CHAIR_COMP1`/`2` (39, 40) and `SCREAM`
/// (124) have no row and play none. `READY` (19) plays the body's wield's
/// ready stance, in the view (`game::acting`).
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
    (20, 349, "deactivate"),
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
    (34, 47, "unlockdr"),
    (35, 457, "meditatesit"),
    // Rows 21 (`Sit_Loop`) and 456 (`Sit_To_Meditate`) share the name; the
    // models hold one `sit` [verify].
    (36, 21, "sit"),
    // `drink` and `cardplay` have no description, and no model holds them
    // [verify].
    (37, 55, "drink"),
    (38, 56, "cardplay"),
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

/// What scripts and conversations play on a body that it may have a clip
/// for, and the clip's name: the constants (`SCRIPTED_CLIPS`) and the
/// conversations' own clips (`DIALOG_CLIPS`). The views draw and the sim
/// times them by this one table.
pub fn scripted_clips() -> impl Iterator<Item = (u16, &'static str)> {
    let dialog = DIALOG_CLIPS.iter().map(|&(_, code, clip)| (code, clip));
    SCRIPTED_CLIPS
        .iter()
        .map(|&(constant, _, clip)| (constant, clip))
        .chain(dialog)
}

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
            let body_variation = uti.root.int("BodyVariation").unwrap_or(1);
            let from_uti = (b'a' as i64 + body_variation - 1).clamp(0, 255) as u8 as char;
            let from_base_item = uti.root.int("BaseItem").and_then(|base| {
                let base_items = tables.get(source, "baseitems")?;
                base_items.resref(usize::try_from(base).ok()?, "bodyvar")
            });
            let disagrees = from_base_item
                .filter(|letter| *letter != from_uti.to_string())
                .map(|letter| (from_uti, letter));
            let variation = uti.root.int("TextureVar").unwrap_or(1);
            (from_uti, variation, disagrees)
        }
    };

    let (body, body_texture) = if model_type == "B" {
        // A row that names no model for the armour's letter (TSL's unique
        // rows fill only `a`) draws its unarmoured body (QUIRKS Q-0137).
        let letter = if appearances.resref(row, &format!("model{letter}")).is_some() {
            letter
        } else {
            'a'
        };
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
            let index = appearances.int(row, column).unwrap_or(-1);
            let heads = heads.as_ref()?;
            usize::try_from(index)
                .ok()
                .and_then(|index| heads.resref(index, "head"))
        };
        head = pick("normalhead");
        if head.is_none() {
            head = pick("backuphead");
            backup_head = head.is_some();
        }
    }
    Ok(CreatureLook {
        right_weapon: None,
        left_weapon: None,
        sound_set: None,
        death_sound: None,
        chest: appearances
            .resref(row, "label")
            .and_then(|label| crate::body_morph::chest(&label)),
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
