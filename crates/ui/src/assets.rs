//! What screens draw with, read from an install: layouts, textures (as
//! upright RGBA), bitmap fonts and the talk table. The same assets feed
//! `kjk gui`'s pictures, the sweep and the game's screens.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::draw::{TextureId, Textures};
use crate::raster::Pixels;
use crate::skin::{self, Source};
use crate::vm::{ItemKind, SceneView, UpgradeKind};
use asset_odyssey::font::Font;
use fmt_odyssey::ResType;
use fmt_odyssey::gui::Gui;
use fmt_odyssey::texture::Texture;
use fmt_odyssey::tlk::Tlk;
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::txi::Txi;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

/// The textures each module has of its own, by these prefixes and its
/// (or its area's) name: its loading screen and its map. Let go when
/// another module is entered.
const MODULE_ART: [&str; 2] = ["load_", "lbl_map"];

/// A loaded texture.
pub struct Loaded {
    pub pixels: Pixels,
    pub additive: bool,
}

/// How an item shows: its name and its icon's texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLook {
    pub name: Vec<u8>,
    /// Its UTI's `Tag`, lowercase (what scripts find it by).
    pub tag: String,
    pub icon: String,
    /// Its value (`Cost`) and what it is (`DescIdentified`).
    pub cost: u32,
    pub description: Vec<u8>,
    /// The `INVENTORY_SLOT_*` bits it may be worn in (its base item's
    /// `equipableslots`).
    pub slots: u32,
    pub kind: ItemKind,
    /// The `spells.2da` row its first Cast Spell property casts (what the
    /// HUD's item slots are by).
    pub casts: Option<u16>,
    /// What an upgrade bench takes it as, if it takes it.
    pub upgrade: Option<UpgradeKind>,
    /// Its base item (`baseitems.2da` row) and plot flag.
    pub base_item: u16,
    pub plot: bool,
}

/// The hands' `INVENTORY_SLOT_*` bits; a creature's own.
const HANDS: u32 = 0x30;
const CREATURE: u32 = 0x3_C000;

/// The install the screens' layouts and art come from (`skin`).
struct Skin {
    game: Game,
    resolver: Resolver,
    tlk: Option<Arc<Tlk>>,
}

pub struct GuiAssets {
    pub game: Game,
    skin: Option<Skin>,
    /// The install's files, its resolver without a module, and the module
    /// whose own files count too.
    index: Arc<DirIndex>,
    base: Resolver,
    module: Option<String>,
    /// Items looked up so far, by template.
    items: BTreeMap<String, Option<ItemLook>>,
    /// Templates' names looked up so far, by resref and type.
    template_names: BTreeMap<(String, ResType), Option<Vec<u8>>>,
    /// Creatures' portraits looked up so far, by template.
    portraits: BTreeMap<String, Option<String>>,
    /// The 3D views screens asked for, by id from `TextureId::SCENES`.
    scenes: Vec<SceneView>,
    /// 2DAs read so far, by name.
    tables: asset_odyssey::tables::Tables,
    resolver: Resolver,
    tlk: Option<Arc<Tlk>>,
    /// By id less one (0 is white); `None` once released.
    loaded: Vec<Option<Loaded>>,
    /// How many times a module's own art was released (`enter_module`).
    released: u32,
    names: BTreeMap<(Source, String), Option<TextureId>>,
    fonts: BTreeMap<(Source, String), Option<LoadedFont>>,
}

type LoadedFont = (TextureId, Arc<Font>);

/// A font name some layouts use for another (both references alias it).
fn font_alias(name: &str) -> &str {
    if name == crate::paint::NUMBER_FONT {
        "fnt_d16x16b"
    } else {
        name
    }
}

impl GuiAssets {
    pub fn open(discovery: &Discovery, game: Game) -> Result<GuiAssets, String> {
        let install = discovery.get(game).ok_or(format!(
            "{}: not found (set {})",
            game.key(),
            game.env_var()
        ))?;
        let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
        let base = Resolver::base(game, &index, TextureQuality::High)
            .map_err(|error| error.to_string())?;
        Ok(GuiAssets::with(game, Arc::new(index), base))
    }

    /// The assets of an install already open: its files and its resolver
    /// without a module (whose talk table they share).
    pub fn with(game: Game, index: Arc<DirIndex>, base: Resolver) -> GuiAssets {
        GuiAssets {
            game,
            skin: None,
            index,
            module: None,
            items: BTreeMap::new(),
            template_names: BTreeMap::new(),
            portraits: BTreeMap::new(),
            scenes: Vec::new(),
            tables: asset_odyssey::tables::Tables::default(),
            resolver: base.clone(),
            tlk: base.talk.get(),
            base,
            loaded: Vec::new(),
            released: 0,
            names: BTreeMap::new(),
            fonts: BTreeMap::new(),
        }
    }

    /// The screens moved to `game`'s layouts and art (`skin`), read with
    /// its resolver without a module (and its talk table).
    pub fn with_skin(mut self, game: Game, base: Resolver) -> GuiAssets {
        self.skin = Some(Skin {
            game,
            tlk: base.talk.get(),
            resolver: base,
        });
        self
    }

    /// The campaign the screens read moved to `game`'s install (its files
    /// and its resolver without a module): its tables, talk table, items
    /// and art from now on (the menu's New Game of TSL, a TSL session); the
    /// skin stays. Nothing changes when it is the campaign already.
    pub fn set_campaign(&mut self, game: Game, index: Arc<DirIndex>, base: Resolver) {
        if game == self.game {
            return;
        }
        self.game = game;
        self.index = index;
        self.tlk = base.talk.get();
        self.resolver = base.clone();
        self.base = base;
        self.module = None;
        self.items.clear();
        self.template_names.clear();
        self.portraits.clear();
        self.tables = asset_odyssey::tables::Tables::default();
        // The campaign's art and fonts are read anew from it.
        let loaded = &mut self.loaded;
        self.names.retain(|(source, _), id| {
            let campaign = *source == Source::Campaign;
            if campaign && let Some(slot) = id.and_then(|id| loaded.get_mut(id.0 as usize - 1)) {
                *slot = None;
            }
            !campaign
        });
        self.fonts
            .retain(|(source, _), _| *source != Source::Campaign);
        self.released += 1;
    }

    /// The game the screens' layouts and art come from.
    fn skin_game(&self) -> Game {
        self.skin.as_ref().map_or(self.game, |skin| skin.game)
    }

    /// The skin when there is one, else the campaign.
    fn side(&self, source: Source) -> Source {
        if self.skin.is_some() {
            source
        } else {
            Source::Campaign
        }
    }

    fn resolver_of(&self, source: Source) -> &Resolver {
        match (source, &self.skin) {
            (Source::Skin, Some(skin)) => &skin.resolver,
            _ => &self.resolver,
        }
    }

    /// The layout a screen named `name` draws: the skin's in its place, but
    /// for one of the campaign's own world (`skin::own`).
    pub fn screen_layout(&self, name: &str) -> Result<(Gui, Source), String> {
        if skin::own(name) {
            return Ok((GuiAssets::layout(self, name)?, Source::Campaign));
        }
        Ok((self.skin_layout(name)?, Source::Skin))
    }

    fn skin_layout(&self, name: &str) -> Result<Gui, String> {
        let skin = self
            .skin
            .as_ref()
            .ok_or("the screens need KOTOR 2's layouts")?;
        let moved = skin::moved(name);
        read_layout(&skin.resolver, &moved).map_err(|error| format!("{name}: {error}"))
    }

    /// The campaign's files with `module`'s archives over them.
    pub fn module_resolver(&self, module: &str) -> Option<Resolver> {
        self.base.with_module(&self.index, module).ok()
    }

    /// The module whose own templates (a container's name) are read too;
    /// nothing changes when it is the one already entered.
    pub fn enter_module(&mut self, module: &str) {
        if self.module.as_deref() == Some(module) {
            return;
        }
        match self.base.with_module(&self.index, module) {
            Ok(resolver) => {
                self.resolver = resolver;
                self.module = Some(module.to_string());
                self.template_names.clear();
                self.portraits.clear();
                self.release_module_art();
            }
            Err(error) => self.module = Some(format!("{module}: {error}")),
        }
    }

    /// A 2DA of the campaign, read once.
    pub fn table(&mut self, table: &str) -> Option<Arc<TwoDa>> {
        let resolver = &self.resolver;
        self.tables.get_with(table, |name| {
            resolver.find(name, ResType::TWO_DA)?.location.read().ok()
        })
    }

    /// The modules' art (`MODULE_ART`) let go: its ids name nothing from
    /// now on, and asked for again (the module entered's) it is read anew.
    /// A module's or area's name has a digit in it, which keeps the art
    /// every module shares (`lbl_mapcircle`).
    fn release_module_art(&mut self) {
        let loaded = &mut self.loaded;
        self.names.retain(|(_, name), id| {
            let art = MODULE_ART.iter().any(|prefix| {
                name.strip_prefix(prefix)
                    .is_some_and(|of| of.bytes().any(|byte| byte.is_ascii_digit()))
            });
            if art && let Some(slot) = id.and_then(|id| loaded.get_mut(id.0 as usize - 1)) {
                *slot = None;
            }
            !art
        });
        self.released += 1;
    }

    /// Changes each time textures were let go: a view holding copies of
    /// them drops those whose `pixels` are gone.
    pub fn released(&self) -> u32 {
        self.released
    }

    /// How many rows a 2DA has (none: 0).
    pub fn rows(&mut self, table: &str) -> usize {
        self.table(table).map_or(0, |twoda| twoda.cells.len())
    }

    /// A 2DA's cell (`****` and blanks are none), lowercased and trimmed.
    pub fn cell(&mut self, table: &str, row: usize, column: &str) -> Option<String> {
        self.cell_as_written(table, row, column)
            .map(|value| value.to_ascii_lowercase())
    }

    /// A 2DA's cell as written, trimmed (a layout's tag it names).
    pub fn cell_as_written(&mut self, table: &str, row: usize, column: &str) -> Option<String> {
        self.table(table)?.text(row, column)
    }

    /// A 2DA cell that is a strref, as its text.
    pub fn cell_text(&mut self, table: &str, row: usize, column: &str) -> Option<Vec<u8>> {
        let strref = u32::try_from(self.table(table)?.int(row, column)?).ok()?;
        self.text(Source::Campaign, strref)
    }

    /// Every layout's name, after precedence.
    pub fn layouts(&self) -> Vec<String> {
        let gui = ResType::GUI;
        self.resolver
            .all()
            .keys()
            .filter(|(_, kind)| *kind == gui)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// The campaign's layout `name`.
    pub fn layout(&self, name: &str) -> Result<Gui, String> {
        read_layout(&self.resolver, name)
    }

    /// How the item with template `resref` shows: its UTI's name (its
    /// strref's text, else its own), and its icon: `i`, its base item's
    /// class, `_`, its variation in three digits (`ModelVariation`, else
    /// `TextureVar`, else 1).
    pub fn item(&mut self, resref: &str) -> Option<ItemLook> {
        self.item_ref(resref).cloned()
    }

    /// The same, borrowed.
    pub fn item_ref(&mut self, resref: &str) -> Option<&ItemLook> {
        if !self.items.contains_key(resref) {
            let look = self.look(resref);
            self.items.insert(resref.to_string(), look);
        }
        self.items.get(resref)?.as_ref()
    }

    /// A template's name (`LocName`, its strref's text else its own; a
    /// creature's first and last names), by its resource type (`utp`,
    /// `utc`…).
    pub fn template_name(&mut self, resref: &str, kind: ResType) -> Option<Vec<u8>> {
        let key = (resref.to_string(), kind);
        if let Some(known) = self.template_names.get(&key) {
            return known.clone();
        }
        let name = self.read_name(resref, kind);
        self.template_names.insert(key, name.clone());
        name
    }

    /// The campaign's files (its module's own first, once one is set).
    pub fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    /// The 3D view `id` names, if it names one.
    pub fn scene_of(&self, id: TextureId) -> Option<&SceneView> {
        self.scenes
            .get(usize::try_from(id.0.checked_sub(TextureId::SCENES)?).ok()?)
    }

    /// A creature template's portrait: its `PortraitId`'s `portraits.2da`
    /// `baseresref`.
    pub fn portrait_of(&mut self, template: &str) -> Option<String> {
        if let Some(known) = self.portraits.get(template) {
            return known.clone();
        }
        let row = self
            .gff(template, ResType::UTC)
            .and_then(|utc| usize::try_from(utc.root.int("PortraitId")?).ok());
        let portrait = row.and_then(|row| self.cell("portraits", row, "baseresref"));
        self.portraits
            .insert(template.to_string(), portrait.clone());
        portrait
    }

    /// The `appearance.2da` row of creature template `template`.
    pub fn appearance_of(&self, template: &str) -> Option<u16> {
        let utc = self.gff(template, ResType::UTC)?;
        u16::try_from(utc.root.int("Appearance_Type")?).ok()
    }

    /// The armour creature template `template` starts with on its body
    /// (`creature::template_armour`).
    pub fn armour_of(&self, template: &str) -> Option<String> {
        asset_odyssey::creature::template_armour(&self.gff(template, ResType::UTC)?)
    }

    /// A GFF resource, read and parsed.
    fn gff(&self, resref: &str, kind: ResType) -> Option<fmt_odyssey::gff::Gff> {
        let bytes = self.resolver.find(resref, kind)?.location.read().ok()?;
        fmt_odyssey::gff::Gff::parse(&bytes).ok()
    }

    /// A localized string's words: its strref's text, else its own.
    fn localized(&self, value: Option<&fmt_odyssey::gff::GffValue>) -> Option<Vec<u8>> {
        match value {
            Some(fmt_odyssey::gff::GffValue::LocString(text)) => (text.strref != u32::MAX)
                .then(|| self.text(Source::Campaign, text.strref))
                .flatten()
                .or_else(|| text.strings.first().map(|(_, words)| words.clone())),
            _ => None,
        }
    }

    fn read_name(&self, resref: &str, kind: ResType) -> Option<Vec<u8>> {
        let gff = self.gff(resref, kind)?;
        if let Some(mut first) = self.localized(gff.root.get("FirstName")) {
            if let Some(last) = self
                .localized(gff.root.get("LastName"))
                .filter(|last| !last.is_empty())
            {
                first.push(b' ');
                first.extend(last);
            }
            return Some(first);
        }
        self.localized(gff.root.get("LocName").or(gff.root.get("Name")))
    }

    fn look(&mut self, resref: &str) -> Option<ItemLook> {
        use asset_odyssey::item::{self, item_type, property};
        let uti = self.gff(resref, ResType::UTI)?;
        let root = &uti.root;
        // TSL's item names carry sort keys in braces its screens leave out.
        let name = self
            .localized(root.get("LocalizedName"))
            .map(|name| fmt_odyssey::tlk::unbraced(&name))
            .unwrap_or_else(|| resref.as_bytes().to_vec());
        let base = root
            .int("BaseItem")
            .and_then(|row| usize::try_from(row).ok());
        let base_items = self.table("baseitems");
        let at = base_items.as_deref().zip(base);
        let class = at
            .and_then(|(table, row)| table.resref(row, "itemclass"))
            .unwrap_or_else(String::new);
        let slots = at
            .and_then(|(table, row)| table.hex(row, "equipableslots"))
            .unwrap_or(0);
        let item_type = at.and_then(|(table, row)| table.int(row, "itemtype"));
        let weapon_type = at.and_then(|(table, row)| table.int(row, "weapontype"));
        let worn_on_body = slots & item::BODY_SLOT != 0;
        let variation = |label| root.int(label).unwrap_or(0);
        let icon = item::icon_name(
            &class,
            worn_on_body,
            variation("ModelVariation"),
            variation("TextureVar"),
        );
        let properties = root.list("PropertiesList");
        let itempropdef = self.table("itempropdef");
        let row_of = |label| {
            itempropdef
                .as_deref()
                .and_then(|table| item::property_row(table, label))
        };
        let named = |label| {
            let row = row_of(label);
            properties
                .iter()
                .filter(move |property| row.is_some() && property.int("PropertyName") == row)
        };
        let casts = named(property::CAST_SPELL)
            .find_map(|property| property.uint("Subtype"))
            .and_then(|row| u16::try_from(row).ok());
        let is = |range: std::ops::RangeInclusive<i64>| {
            item_type.is_some_and(|kind| range.contains(&kind))
        };
        // A bench takes a hand's weapon (a lightsaber or ranged by its base
        // item's typed columns, else melee) or body armour.
        let upgrade = if slots & CREATURE != 0 {
            None
        } else if slots & HANDS != 0 {
            Some(if is(item_type::LIGHTSABER) {
                UpgradeKind::Lightsaber
            } else if weapon_type == Some(item::RANGED_WEAPON_TYPE) {
                UpgradeKind::Ranged
            } else {
                UpgradeKind::Melee
            })
        } else if slots == item::BODY_SLOT && is(item_type::ARMOUR) {
            Some(UpgradeKind::Armour)
        } else {
            None
        };
        let kind = if item_type == Some(item_type::DATAPAD) {
            ItemKind::Datapad
        } else if slots & HANDS != 0 {
            ItemKind::Weapon
        } else if slots != 0 {
            ItemKind::Armour
        } else if casts.is_some() || named(property::TRAP).next().is_some() {
            ItemKind::Useable
        } else if root.int("Plot").is_some_and(|plot| plot != 0) {
            ItemKind::Quest
        } else {
            ItemKind::Misc
        };
        Some(ItemLook {
            name,
            tag: root.resref("Tag").unwrap_or_else(String::new),
            icon,
            cost: root.uint("Cost").unwrap_or(0),
            description: self
                .localized(root.get("DescIdentified"))
                .unwrap_or(Vec::new()),
            slots,
            kind,
            casts,
            upgrade,
            base_item: base.and_then(|row| u16::try_from(row).ok()).unwrap_or(0),
            plot: root.int("Plot").is_some_and(|plot| plot != 0),
        })
    }

    pub fn pixels(&self, id: TextureId) -> Option<&Pixels> {
        let index = (id.0 as usize).checked_sub(1)?;
        self.loaded
            .get(index)?
            .as_ref()
            .map(|loaded| &loaded.pixels)
    }

    fn read(&self, source: Source, name: &str) -> Option<(Texture, Txi)> {
        let resolver = self.resolver_of(source);
        let loose = resolver
            .find(name, ResType::TXI)
            .and_then(|found| found.location.read().ok())
            .map(|bytes| Txi::parse(&bytes));
        let texture = match resolver.find(name, ResType::TPC) {
            Some(found) => Texture::parse_tpc(&found.location.read().ok()?).ok()?,
            None => {
                let found = resolver.find(name, ResType::TGA)?;
                Texture::parse_tga(&found.location.read().ok()?).ok()?
            }
        };
        let txi = loose.unwrap_or_else(|| texture.txi.clone());
        Some((texture, txi))
    }

    /// `name` read from `source`; a font's sheet is a shape (`Pixels`).
    fn load(&mut self, source: Source, name: &str, font: bool) -> Option<(TextureId, Txi)> {
        let (texture, txi) = self.read(source, name)?;
        let mut rgba = texture.rgba(0, 0)?;
        if self.recoloured(source) {
            skin::recolour_pixels(&mut rgba);
        }
        let additive = txi
            .get("blending")
            .and_then(|values| values.first())
            .is_some_and(|mode| mode.eq_ignore_ascii_case("additive"));
        self.loaded.push(Some(Loaded {
            pixels: Pixels {
                width: texture.width() as u32,
                height: texture.height() as u32,
                shape: font || crate::raster::is_shape(&rgba),
                rgba,
            },
            additive,
        }));
        Some((TextureId(self.loaded.len() as u32), txi))
    }
}

/// What screens draw from: textures, fonts, text, and layouts by name.
pub trait Assets: Textures {
    /// The layout a screen named `name` draws, and where it was read.
    fn layout(&mut self, name: &str) -> Result<(Gui, Source), String>;
    /// The layout `name` as `source` has it (one drawn under a screen's).
    fn layout_in(&mut self, name: &str, source: Source) -> Result<Gui, String>;
}

impl Assets for GuiAssets {
    fn layout(&mut self, name: &str) -> Result<(Gui, Source), String> {
        self.screen_layout(name)
    }

    fn layout_in(&mut self, name: &str, source: Source) -> Result<Gui, String> {
        match source {
            Source::Skin => self.skin_layout(name),
            Source::Campaign => GuiAssets::layout(self, name),
        }
    }
}

impl Textures for GuiAssets {
    fn scene(&mut self, view: &SceneView) -> Option<TextureId> {
        let at = match self.scenes.iter().position(|known| known == view) {
            Some(at) => at,
            None => {
                self.scenes.push(view.clone());
                self.scenes.len() - 1
            }
        };
        Some(TextureId(TextureId::SCENES + u32::try_from(at).ok()?))
    }

    fn text(&self, source: Source, strref: u32) -> Option<Vec<u8>> {
        let tlk = match (source, &self.skin) {
            (Source::Skin, Some(skin)) => skin.tlk.as_ref(),
            _ => self.tlk.as_ref(),
        };
        tlk?.text(strref).map(<[u8]>::to_vec)
    }

    fn texture(&mut self, source: Source, name: &str) -> Option<TextureId> {
        let key = (self.side(source), name.to_ascii_lowercase());
        if let Some(known) = self.names.get(&key) {
            return *known;
        }
        let id = self.load(key.0, &key.1, false).map(|(id, _)| id);
        self.names.insert(key, id);
        id
    }

    fn size(&self, id: TextureId) -> (u32, u32) {
        self.pixels(id)
            .map_or((1, 1), |pixels| (pixels.width, pixels.height))
    }

    fn additive(&self, id: TextureId) -> bool {
        let index = (id.0 as usize).wrapping_sub(1);
        self.loaded
            .get(index)
            .is_some_and(|loaded| loaded.as_ref().is_some_and(|loaded| loaded.additive))
    }

    fn font(&mut self, source: Source, name: &str) -> Option<(TextureId, Arc<Font>)> {
        let key = (
            self.side(source),
            font_alias(&name.to_ascii_lowercase()).to_string(),
        );
        if let Some(known) = self.fonts.get(&key) {
            return known.clone();
        }
        let font = self.load(key.0, &key.1, true).and_then(|(id, txi)| {
            let (width, height) = self.size(id);
            let font = Font::from_txi(&key.1, &txi, width, height).ok()?;
            Some((id, Arc::new(font)))
        });
        self.fonts.insert(key, font.clone());
        font
    }

    /// KOTOR 2's art over a KOTOR 1 campaign.
    fn recoloured(&self, source: Source) -> bool {
        self.side(source) == Source::Skin
            && self.skin_game() == Game::Kotor2
            && self.game == Game::Kotor1
    }
}

fn read_layout(resolver: &Resolver, name: &str) -> Result<Gui, String> {
    let found = resolver
        .find(name, ResType::GUI)
        .ok_or(format!("{name}.gui: not found"))?;
    let bytes = found.location.read().map_err(|error| error.to_string())?;
    Gui::parse(&bytes).map_err(|error| format!("{name}.gui: {error}"))
}
