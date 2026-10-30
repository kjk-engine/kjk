//! The templates the import step writes once per install (`docs/IMPORT.md`,
//! part 2): every item template and every creature template, as the sim
//! uses them, in one fixed encoding the store keeps; and a module's
//! gathered scripts and conversations (`Gathered`, slice 3).

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_odyssey::dlg::{Call, Dlg, Kind, Link, Node, NodeFade};
use fmt_odyssey::gff::LocString;
use fmt_odyssey::ncs::Ncs;

use crate::combat::Sheet;
use crate::combat::sheet::{Guards, decode_weapon, encode_weapon};
use crate::inventory::{Armour, Catalog, Held, ItemFacts, ItemSpell};
use crate::module::{ObjectData, ObjectKind, Opens, Senses, Strike};
use crate::wire::{DecodeError, Reader, Versions, Writer, read_fixed, write_fixed};

const MAGIC: &[u8; 4] = b"KJKT";
/// Bump when the encoding or what a template carries changes. 8: sheets
/// carry their alignment. 9: creatures their body (`module::Body`). 10:
/// creatures their hit points. 11: items what a use of them costs
/// (`ItemFacts::spell`). 12: and who may use them (`ItemFacts::droids`).
/// 13: the upgrades they take, are, or the lightsabers a colour crystal
/// makes (`ItemFacts::upgrades`, `upgrade`, `recolours`). 14: the feats
/// they need, weapons how they are held (`ItemFacts::needs`,
/// `Weapon::build`). 15: their skill bonuses, the trap a kit lays and
/// whether a stealth unit; sheets what items add to skills. 16: items
/// their base weapon and every property in force (`ItemFacts::bare`,
/// `properties`, `rows`; the rest made of them), TSL's upgrade level and
/// slots; sheets what items give (`Sheet::gear`). 17: creatures' bodies
/// what their deaths leave (`module::Body::bag`, its delays). 18: whether
/// creatures are interruptable (`ObjectData::interruptable`). 19:
/// creatures' bodies their walk and run speeds and head arc
/// (`module::Body::walk_mps`, `run_mps`, `head_arc_h`). 20: items whether
/// an energy shield (`ItemFacts::shield`).
/// 21: a creature template carries TSL's MultiplierSet, its autobalance.2da row
/// (`ObjectData::multiplier_set`). Old saves: old saves read a kept companion's
/// MultiplierSet as 0: no companion is autobalanced.
pub const VERSION: u32 = 21 + crate::pending::PACK;

/// The oldest version read in a save's companions' templates: v0.1.0's
/// (`crate::since::pack`, `bumps/README.md`).
pub const OLDEST: u32 = crate::wire::later(20, crate::pending::oldest::PACK);

/// Item and creature templates by lowercase resref (the creatures' shared:
/// a plan hands them to its module and its library as they are).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Templates {
    pub items: Catalog,
    pub creatures: BTreeMap<String, Arc<ObjectData>>,
    /// The item templates each creature's lists name, by creature, those
    /// the shared data lacks too: a module that gives its own copy of one
    /// dresses the creature differently.
    pub named_items: BTreeMap<String, Vec<String>>,
}

impl Templates {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.u32(self.items.len() as u32);
        for facts in self.items.iter() {
            encode_item(facts, &mut out);
        }
        out.u32(self.creatures.len() as u32);
        for (name, facts) in &self.creatures {
            out.string(name);
            encode_creature(facts, &mut out);
        }
        out.u32(self.named_items.len() as u32);
        for (name, items) in &self.named_items {
            out.string(name);
            encode_list(items, &mut out, |item, out| out.string(item));
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Templates, DecodeError> {
        let mut input = Reader::new("templates", bytes);
        input.header(MAGIC, VERSION)?;
        let items = (0..input.u32()?)
            .map(|_| decode_item(&mut input))
            .collect::<Result<Vec<_>, _>>()?;
        let mut creatures = BTreeMap::new();
        for _ in 0..input.u32()? {
            let name = input.string()?;
            creatures.insert(name, Arc::new(decode_creature(&mut input)?));
        }
        let mut named_items = BTreeMap::new();
        for _ in 0..input.u32()? {
            let name = input.string()?;
            named_items.insert(name, decode_list(&mut input, |input| input.string())?);
        }
        input.finish()?;
        Ok(Templates {
            items: Catalog::new(items),
            creatures,
            named_items,
        })
    }
}

/// An item's facts; its tag as its data writes it (lowercase looked up,
/// its capitals for `GetTag`).
pub(crate) fn encode_item(facts: &ItemFacts, out: &mut Writer) {
    out.string(&facts.template);
    out.string(facts.written_tag());
    out.u32(facts.name);
    out.u16(facts.base_item);
    out.u16(facts.stack);
    out.u16(facts.stacking);
    out.u8(facts.charges);
    out.u32(facts.cost);
    out.u8(u8::from(facts.plot));
    out.u32(facts.slots);
    write_fixed(
        out,
        facts.bare.as_ref(),
        &crate::combat::Weapon::UNARMED,
        encode_weapon,
    );
    write_fixed(out, facts.armour.as_ref(), &Armour::NONE, |armour, out| {
        out.i8(armour.defence);
        out.u8(armour.dex_cap.unwrap_or(u8::MAX));
    });
    encode_list(&facts.properties, out, |property, out| property.encode(out));
    out.u64(facts.rows as u64);
    out.u64((facts.rows >> 64) as u64);
    write_fixed(
        out,
        facts.spell.as_ref(),
        &ItemSpell { row: 0, cost: 0 },
        |spell, out| {
            out.u16(spell.row);
            out.u8(spell.cost);
        },
    );
    out.u8(facts.tools);
    out.u8(facts.droids.map_or(0, |droids| 1 + u8::from(droids)));
    encode_list(&facts.upgrades, out, |upgrade, out| upgrade.encode(out));
    let (row, kind) = facts.upgrade.unwrap_or((u16::MAX, u8::MAX));
    out.u16(row);
    out.u8(kind);
    out.u8(facts.upgrade_level);
    encode_list(&facts.slot_kinds, out, |kind, out| out.u8(*kind));
    encode_list(&facts.parts, out, |part, out| out.string(part));
    encode_list(&facts.recolours, out, |template, out| out.string(template));
    encode_list(&facts.needs, out, |feat, out| out.u16(*feat));
    out.u8(facts.trap.unwrap_or(u8::MAX));
    out.u8(u8::from(facts.stealth_unit) | u8::from(facts.shield) << 1);
}

pub(crate) fn decode_item(input: &mut Reader) -> Result<ItemFacts, DecodeError> {
    let template = input.string()?;
    let written = input.string()?;
    let mut facts = ItemFacts {
        template,
        tag: written.to_ascii_lowercase(),
        cased_tag: crate::module::cased(&written),
        name: input.u32()?,
        base_item: input.u16()?,
        stack: input.u16()?,
        stacking: input.u16()?,
        charges: input.u8()?,
        cost: input.u32()?,
        plot: input.u8()? != 0,
        slots: input.u32()?,
        bare: read_fixed(
            input,
            &crate::combat::Weapon::UNARMED,
            decode_weapon,
            "weapon",
        )?,
        weapon: None,
        armour: read_fixed(
            input,
            &Armour::NONE,
            |input| {
                Ok(Armour {
                    defence: input.i8()?,
                    dex_cap: Some(input.u8()?).filter(|cap| *cap != u8::MAX),
                })
            },
            "armour",
        )?,
        guards: Guards::default(),
        properties: decode_list(input, crate::inventory::Property::decode)?,
        rows: u128::from(input.u64()?) | u128::from(input.u64()?) << 64,
        gear: Default::default(),
        limits: Vec::new(),
        spell: read_fixed(
            input,
            &ItemSpell { row: 0, cost: 0 },
            |input| {
                Ok(ItemSpell {
                    row: input.u16()?,
                    cost: input.u8()?,
                })
            },
            "spell",
        )?,
        tools: input.u8()?,
        droids: match input.u8()? {
            0 => None,
            byte => Some(byte == 2),
        },
        upgrades: decode_list(input, crate::inventory::Upgrade::decode)?,
        upgrade: Some((input.u16()?, input.u8()?)).filter(|(row, _)| *row != u16::MAX),
        upgrade_level: input.u8()?,
        slot_kinds: decode_list(input, |input| input.u8())?,
        parts: decode_list(input, |input| input.string())?,
        recolours: decode_list(input, |input| input.string())?,
        needs: decode_list(input, |input| input.u16())?,
        trap: Some(input.u8()?).filter(|row| *row != u8::MAX),
        stealth_unit: false,
        shield: false,
    };
    let kinds = input.u8()?;
    facts.stealth_unit = kinds & 1 != 0;
    facts.shield = kinds & 2 != 0;
    Ok(facts.made())
}

/// A creature template's facts as bytes of their own (the party table
/// keeps a companion's, `script::party::Npc::template_data`).
pub(crate) fn creature_bytes(facts: &ObjectData) -> Vec<u8> {
    let mut out = Writer::default();
    encode_creature(facts, &mut out);
    out.0
}

/// The creature template `creature_bytes` made, in a save of `versions`
/// (`Versions::THIS_BUILD` for this build's).
pub(crate) fn creature_of(bytes: &[u8], versions: Versions) -> Result<ObjectData, DecodeError> {
    decode_creature(&mut Reader::new("a kept companion", bytes).with_versions(versions))
}

/// A creature template: what a creature made from it has (no place, no
/// link, no outline: those are a placement's).
fn encode_creature(facts: &ObjectData, out: &mut Writer) {
    out.string(&facts.tag);
    out.string(&facts.cased_tag);
    out.string(&facts.template);
    out.u32(facts.scripts.len() as u32);
    for (hook, script) in &facts.scripts {
        out.string(hook);
        out.string(script);
    }
    match &facts.sheet {
        Some(sheet) => {
            out.u8(1);
            sheet.encode(out);
        }
        None => out.u8(0),
    }
    match &facts.senses {
        Some(senses) => {
            out.u8(1);
            out.f32(senses.sight);
            out.f32(senses.hearing);
        }
        None => out.u8(0),
    }
    out.u32(facts.classes.len() as u32);
    for (class, levels) in &facts.classes {
        out.u8(*class);
        out.u8(*levels);
    }
    out.u8(facts.gender);
    out.u8(facts.subrace);
    out.f32(facts.personal_space);
    write_fixed(out, facts.strike.as_ref(), &Strike::BLANK, Strike::encode);
    out.f32(facts.challenge);
    out.u8(facts.multiplier_set);
    out.string(&facts.name);
    out.u32(facts.inventory.len() as u32);
    for held in &facts.inventory {
        out.string(&held.template);
        out.u16(held.stack);
        out.u8(held.slot.unwrap_or(u8::MAX));
        out.u8(u8::from(held.droppable));
    }
    out.string(&facts.conversation);
    match &facts.body {
        Some(body) => {
            out.u8(1);
            body.encode(out);
        }
        None => out.u8(0),
    }
    match facts.hit_points {
        Some((now, most)) => {
            out.u8(1);
            out.u32(now as u32);
            out.u32(most as u32);
        }
        None => out.u8(0),
    }
    out.u8(u8::from(facts.interruptable));
}

fn decode_creature(input: &mut Reader) -> Result<ObjectData, DecodeError> {
    let tag = input.string()?;
    let cased_tag = input.string()?;
    let template = input.string()?;
    let scripts = (0..input.u32()?)
        .map(|_| Ok((input.string()?, input.string()?)))
        .collect::<Result<_, DecodeError>>()?;
    let at = input.at();
    let sheet = match input.u8()? {
        0 => None,
        1 => Some(Sheet::decode(input)?),
        _ => return Err(input.invalid("sheet flag", at)),
    };
    let at = input.at();
    let senses = match input.u8()? {
        0 => None,
        1 => Some(Senses {
            sight: input.f32()?,
            hearing: input.f32()?,
        }),
        _ => return Err(input.invalid("senses flag", at)),
    };
    let classes = (0..input.u32()?)
        .map(|_| Ok((input.u8()?, input.u8()?)))
        .collect::<Result<_, DecodeError>>()?;
    let gender = input.u8()?;
    let subrace = input.u8()?;
    let personal_space = input.f32()?;
    let strike = read_fixed(input, &Strike::BLANK, Strike::decode, "strike flag")?;
    let challenge = input.f32()?;
    // TSL's `MultiplierSet` (`ObjectData::multiplier_set`), kept since the
    // finale's bump; none before.
    let multiplier_set = if input.since(crate::since::pack::CLAUDE_TSL_FINALE) {
        input.u8()?
    } else {
        0
    };
    let name = input.string()?;
    let inventory = (0..input.u32()?)
        .map(|_| {
            Ok(Held {
                template: input.string()?,
                stack: input.u16()?,
                slot: Some(input.u8()?).filter(|slot| *slot != u8::MAX),
                droppable: input.u8()? != 0,
                infinite: false,
            })
        })
        .collect::<Result<_, DecodeError>>()?;
    let mut facts = ObjectData {
        kind: ObjectKind::Creature,
        tag,
        cased_tag,
        template,
        position: [0.0; 3],
        yaw: 0.0,
        link: None,
        geometry: Vec::new(),
        use_points: Vec::new(),
        opens: Opens::Never,
        state: 0,
        ways: crate::module::Ways::One,
        scripts,
        sheet,
        senses,
        classes,
        gender,
        subrace,
        personal_space,
        strike,
        challenge,
        multiplier_set,
        name,
        inventory,
        container: false,
        useable: false,
        store: None,
        lock: None,
        hit_points: None,
        toughness: crate::module::Toughness::default(),
        faction: 0,
        trap: None,
        conversation: input.string()?,
        body: match input.u8()? {
            0 => None,
            _ => Some(crate::module::Body::decode(input)?),
        },
        map_note: None,
        interruptable: true,
        pitch_variation: 0.0,
        encounter: None,
    };
    if input.u8()? != 0 {
        facts.hit_points = Some((input.u32()? as i32, input.u32()? as i32));
    }
    facts.interruptable = input.u8()? != 0;
    Ok(facts)
}

const GATHERED_MAGIC: &[u8; 4] = b"KJKL";
/// Bump when the encoding or what a gathering carries changes. 2: nodes
/// carry their whole fade (`NodeFade`). 3: nodes their camera's and its
/// target's height offsets, conversations `OldHitCheck`, `UnequipItems` and
/// `UnequipHItem`. 4: nodes TSL's `NodeUnskippable`. 5: nodes TSL's
/// `AlienRaceNode` and `Emotion`, conversations `AlienRaceOwner`. 6: nodes
/// their camera's video effect (`CamVidEffect`).
pub const GATHERED_VERSION: u32 = 6 + crate::pending::GATHERED;

/// What a module's script library is made of (`session`'s gathering): its
/// compiled scripts, its conversations and the strings the scripts hold.
/// Kept in the store, a plan reads it back instead of walking the install
/// and parsing every DLG.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gathered {
    /// By lowercase resref.
    pub scripts: BTreeMap<String, Ncs>,
    /// By lowercase resref.
    pub dialogues: BTreeMap<String, Dlg>,
    /// Every resref-like string the scripts hold, sorted, without repeats.
    pub strings: Vec<String>,
}

impl Gathered {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        out.bytes(GATHERED_MAGIC);
        out.u32(GATHERED_VERSION);
        out.u32(self.scripts.len() as u32);
        for (name, ncs) in &self.scripts {
            out.string(name);
            out.section(&ncs.write());
        }
        out.u32(self.dialogues.len() as u32);
        for (name, dlg) in &self.dialogues {
            out.string(name);
            encode_dlg(dlg, &mut out);
        }
        out.u32(self.strings.len() as u32);
        for string in &self.strings {
            out.string(string);
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Gathered, DecodeError> {
        let mut input = Reader::new("gathered scripts", bytes);
        input.header(GATHERED_MAGIC, GATHERED_VERSION)?;
        let mut scripts = BTreeMap::new();
        for _ in 0..input.u32()? {
            let name = input.string()?;
            let at = input.at();
            let ncs = Ncs::parse(input.section()?).map_err(|_| input.invalid("script", at))?;
            scripts.insert(name, ncs);
        }
        let mut dialogues = BTreeMap::new();
        for _ in 0..input.u32()? {
            let name = input.string()?;
            dialogues.insert(name, decode_dlg(&mut input)?);
        }
        let strings = (0..input.u32()?)
            .map(|_| input.string())
            .collect::<Result<_, _>>()?;
        input.finish()?;
        Ok(Gathered {
            scripts,
            dialogues,
            strings,
        })
    }
}

fn encode_list<T>(items: &[T], out: &mut Writer, encode: fn(&T, &mut Writer)) {
    out.u32(items.len() as u32);
    for item in items {
        encode(item, out);
    }
}

fn decode_list<T>(
    input: &mut Reader,
    decode: fn(&mut Reader) -> Result<T, DecodeError>,
) -> Result<Vec<T>, DecodeError> {
    (0..input.u32()?).map(|_| decode(input)).collect()
}

/// A conversation as the sim runs it: what a library's digest is of.
pub(crate) fn encode_dlg(dlg: &Dlg, out: &mut Writer) {
    encode_list(&dlg.entries, out, encode_node);
    encode_list(&dlg.replies, out, encode_node);
    encode_list(&dlg.starts, out, encode_link);
    out.u8(match dlg.kind {
        Kind::Cinematic => 0,
        Kind::Computer => 1,
    });
    out.u8(dlg.computer);
    out.u8(u8::from(dlg.skippable));
    out.string(&dlg.on_end);
    out.string(&dlg.on_abort);
    out.string(&dlg.camera_model);
    out.u8(u8::from(dlg.animated_cut)
        | u8::from(dlg.old_hit_check) << 1
        | u8::from(dlg.unequip_items) << 2
        | u8::from(dlg.unequip_held) << 3);
    encode_list(&dlg.stunts, out, |(tag, model), out| {
        out.string(tag);
        out.string(model);
    });
    out.u32(dlg.alien_race_owner as u32);
}

fn decode_dlg(input: &mut Reader) -> Result<Dlg, DecodeError> {
    let entries = decode_list(input, decode_node)?;
    let replies = decode_list(input, decode_node)?;
    let starts = decode_list(input, decode_link)?;
    let at = input.at();
    let kind = match input.u8()? {
        0 => Kind::Cinematic,
        1 => Kind::Computer,
        _ => return Err(input.invalid("conversation kind", at)),
    };
    let (computer, skippable) = (input.u8()?, input.u8()? != 0);
    let (on_end, on_abort) = (input.string()?, input.string()?);
    let (camera_model, flags) = (input.string()?, input.u8()?);
    Ok(Dlg {
        entries,
        replies,
        starts,
        kind,
        computer,
        skippable,
        on_end,
        on_abort,
        camera_model,
        animated_cut: flags & 1 != 0,
        old_hit_check: flags & 2 != 0,
        unequip_items: flags & 4 != 0,
        unequip_held: flags & 8 != 0,
        stunts: decode_list(input, |input| Ok((input.string()?, input.string()?)))?,
        alien_race_owner: input.u32()? as i32,
    })
}

fn encode_node(node: &Node, out: &mut Writer) {
    out.u32(node.text.strref);
    encode_list(&node.text.strings, out, |(language, text), out| {
        out.u32(*language);
        out.section(text);
    });
    out.string(&node.speaker);
    out.string(&node.listener);
    encode_call(&node.script, out);
    encode_call(&node.script2, out);
    out.string(&node.vo);
    out.string(&node.sound);
    out.u32(node.delay.unwrap_or(u32::MAX));
    out.u32(node.wait_flags);
    out.u32(node.camera_angle);
    out.u32(node.camera_id as u32);
    out.u16(node.camera_animation);
    out.f32(node.camera_fov);
    out.f32(node.cam_height);
    out.f32(node.target_height);
    out.u32(node.cam_vid_effect as u32);
    out.u8(node.fade.kind);
    out.f32(node.fade.delay);
    out.f32(node.fade.length);
    for channel in node.fade.colour {
        out.f32(channel);
    }
    out.string(&node.quest);
    out.u32(node.quest_entry);
    out.u32(node.plot_index as u32);
    out.f32(node.plot_xp);
    encode_list(&node.anims, out, |(tag, anim), out| {
        out.string(tag);
        out.u16(*anim);
    });
    out.u8(u8::from(node.unskippable));
    out.u32(node.alien_race as u32);
    out.u32(node.emotion as u32);
    encode_list(&node.links, out, encode_link);
}

fn decode_node(input: &mut Reader) -> Result<Node, DecodeError> {
    Ok(Node {
        text: LocString {
            strref: input.u32()?,
            strings: decode_list(input, |input| Ok((input.u32()?, input.section()?.to_vec())))?,
        },
        speaker: input.string()?,
        listener: input.string()?,
        script: decode_call(input)?,
        script2: decode_call(input)?,
        vo: input.string()?,
        sound: input.string()?,
        delay: Some(input.u32()?).filter(|delay| *delay != u32::MAX),
        wait_flags: input.u32()?,
        camera_angle: input.u32()?,
        camera_id: input.u32()? as i32,
        camera_animation: input.u16()?,
        camera_fov: input.f32()?,
        cam_height: input.f32()?,
        target_height: input.f32()?,
        cam_vid_effect: input.u32()? as i32,
        fade: NodeFade {
            kind: input.u8()?,
            delay: input.f32()?,
            length: input.f32()?,
            colour: [input.f32()?, input.f32()?, input.f32()?],
        },
        quest: input.string()?,
        quest_entry: input.u32()?,
        plot_index: input.u32()? as i32,
        plot_xp: input.f32()?,
        anims: decode_list(input, |input| Ok((input.string()?, input.u16()?)))?,
        unskippable: input.u8()? != 0,
        alien_race: input.u32()? as i32,
        emotion: input.u32()? as i32,
        links: decode_list(input, decode_link)?,
    })
}

fn encode_link(link: &Link, out: &mut Writer) {
    out.u32(link.index);
    encode_call(&link.active, out);
    encode_call(&link.active2, out);
    out.u32(link.logic as u32);
    out.u8(u8::from(link.not));
    out.u8(u8::from(link.not2));
    out.u8(u8::from(link.is_child));
}

fn decode_link(input: &mut Reader) -> Result<Link, DecodeError> {
    Ok(Link {
        index: input.u32()?,
        active: decode_call(input)?,
        active2: decode_call(input)?,
        logic: input.u32()? as i32,
        not: input.u8()? != 0,
        not2: input.u8()? != 0,
        is_child: input.u8()? != 0,
    })
}

fn encode_call(call: &Call, out: &mut Writer) {
    out.string(&call.script);
    for int in call.ints {
        out.u32(int as u32);
    }
    out.section(&call.string);
}

fn decode_call(input: &mut Reader) -> Result<Call, DecodeError> {
    let script = input.string()?;
    let mut ints = [0; 5];
    for int in &mut ints {
        *int = input.u32()? as i32;
    }
    Ok(Call {
        script,
        ints,
        string: input.section()?.to_vec(),
    })
}
