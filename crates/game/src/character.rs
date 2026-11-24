//! What the in-game screens show of the player's own character (render
//! side only: the presented snapshot's avatar, its sheet and fighter, and
//! the module's area), with the game's own names and icons from its 2DAs:
//!
//! - skills (`skills.2da` `name`, `icon`, `description`) at the sheet's
//!   ranks; powers known (`spells.2da` `name`, `iconresref`, `spelldesc`);
//!   feats known (`feat.2da` `name`, `icon`, `description`);
//! - the weapons by their base item (`baseitems.2da` `name`, the icon of
//!   its class); what the party carries and what the character wears, from
//!   the snapshot's goods (their templates' names, icons, descriptions and
//!   slots), and the party's credits;
//! - defence as 10, the DEX modifier (capped by the armour) and the
//!   sheet's; saves with their abilities; damage dice per hand;
//! - the map: the area's name and its map texture;
//! - the name and portrait chosen in character creation (`Made`), while
//!   the sheet is still the default one;
//! - character creation's choices: the classes the game's creation offers
//!   (`classes.2da`: K1's three, TSL's three Jedi; name, hit die, skill
//!   points and how a level counts them, recommended scores, level one's saves
//!   from its saving throw table, its skills and their order from
//!   `skills.2da`'s `<table>_class` and `_reco`), the player portraits of
//!   `portraits.2da` (`forpc`), the skills' descriptions.
//!
//! Built only while a screen that shows them is open.

use bevy::prelude::*;
use sim::combat::Sheet;
use sim::inventory::properties::Wearer;
use sim::inventory::{Catalog, Goods, Item, ItemFacts, PARTY, has_upgrade};
use sim::script::Library;
use ui::assets::{GuiAssets, ItemLook};
use ui::draw::Textures as _;
use ui::intent::CharacterDesc;
use ui::screens::action_bar::ActionBar;
use ui::screens::tab::Page;
use ui::skin::Source;
use ui::vm::{
    AutoLevelView, CharacterView, ChargenView, ClassView, EffectIcon, Entry, ItemKind, ItemText,
    MemberView, NameTables, Owned, PicksView, PortraitView, Preview, QuestView, UpgradeChoice,
    UpgradeItem, UpgradePart, UpgradeSlot,
};

use crate::rulebook_text;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The character made in character creation, for this session.
#[derive(Resource, Default)]
pub struct Made(pub Option<CharacterDesc>);

/// The character's view and creation's choices are filled.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Viewed;

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Made>()
            .init_resource::<crate::workbench_view::OpenBench>()
            .add_systems(
                Update,
                (show, choices)
                    .in_set(Viewed)
                    .before(crate::ui_view::Stepped),
            );
    }
}

/// "Male", "Female": the same strrefs in both games (reone's class
/// selection).
const GENDERS: [u32; 2] = [646, 647];
const SAVES: [&str; 3] = ["fortsave", "refsave", "willsave"];

fn number<T: std::str::FromStr>(value: Option<String>) -> Option<T> {
    value?.parse().ok()
}

/// `feat.2da`'s and `spells.2da`'s name, icon and description columns.
const FEAT: [&str; 3] = ["name", "icon", "description"];
const POWER: [&str; 3] = ["name", "iconresref", "spelldesc"];

/// What a level may pick, named as the pages name feats and powers.
pub fn picks_view(assets: &mut GuiAssets, picks: &sim::levels::Picks) -> PicksView {
    let mut name =
        |table: &str, columns, row: u16| (row, entry(assets, table, usize::from(row), columns));
    let mut named = |table: &str, columns, rows: &[u16]| -> Vec<(u16, Entry)> {
        rows.iter().map(|&row| name(table, columns, row)).collect()
    };
    let feats = named("feat", FEAT, &picks.feats);
    let powers = named("spells", POWER, &picks.powers);
    let mut chart = |table: &str, columns, rows: &[[Option<u16>; 3]]| {
        rows.iter()
            .map(|row| row.map(|at| Some(name(table, columns, at?))))
            .collect()
    };
    PicksView {
        feats,
        feat_count: picks.feat_count,
        powers,
        power_count: picks.power_count,
        chart: chart("feat", FEAT, &picks.chart),
        known: picks.known.clone(),
        power_chart: chart("spells", POWER, &picks.power_chart),
        known_powers: picks.known_powers.clone(),
        class_skills: picks.class_skills,
        feat_needs: picks.feat_needs.clone(),
        power_needs: picks.power_needs.clone(),
    }
}

fn class(assets: &mut GuiAssets, classes: &sim::levels::Classes, row: usize) -> ClassView {
    let saves = assets.cell("classes", row, "savingthrowtable");
    let skills = assets
        .cell("classes", row, "skillstable")
        .unwrap_or(String::new());
    let skill_count = assets.rows("skills").min(8);
    let mut order: Vec<(u8, usize)> = (0..skill_count)
        .filter_map(|skill| {
            let rank = number::<u8>(assets.cell("skills", skill, &format!("{skills}_reco")))?;
            (rank > 0).then_some((rank, skill))
        })
        .collect();
    order.sort();
    let mut view = ClassView {
        name: assets
            .cell_text("classes", row, "name")
            .unwrap_or(Vec::new()),
        hit_die: number(assets.cell("classes", row, "hitdie")).unwrap_or(0),
        skill_points: number(assets.cell("classes", row, "skillpointbase")).unwrap_or(0),
        // The host checks a creation against these very scores.
        abilities: classes
            .classes
            .get(row)
            .map_or([sim::creation::LOWEST; 6], |class| class.recommended),
        primary: classes.classes.get(row).map_or(0, |class| class.primary),
        saves: SAVES.map(|column| {
            saves
                .as_ref()
                .and_then(|table| number(assets.cell(table, 0, column)))
                .unwrap_or(0)
        }),
        class_skills: std::array::from_fn(|skill| {
            assets
                .cell("skills", skill, &format!("{skills}_class"))
                .as_deref()
                == Some("1")
        }),
        skill_order: order.into_iter().map(|(_, skill)| skill).collect(),
        picks: PicksView::default(),
    };
    // Its first level's picks, from the scores it recommends (no feat asks
    // for a score).
    if let Some(picks) = u8::try_from(row).ok().and_then(|row| {
        let unmade = sim::creation::unmade_of(classes, row, view.abilities, 0);
        classes.picks(&unmade, &Vec::new(), row)
    }) {
        view.picks = picks_view(assets, &picks);
    }
    view
}

pub fn chargen(assets: &mut GuiAssets) -> ChargenView {
    let portraits = (0..assets.rows("portraits"))
        .filter_map(|row| {
            (assets.cell("portraits", row, "forpc").as_deref() == Some("1")).then_some(())?;
            Some(PortraitView {
                texture: assets.cell("portraits", row, "baseresref")?,
                female: assets.cell("portraits", row, "sex").as_deref() == Some("1"),
            })
        })
        .collect();
    let source = crate::viewer::ResolverSource(assets.resolver());
    let classes = session::classes::classes(&source);
    let ltr = |name: &str| {
        let bytes = asset_odyssey::module::Source::read(
            &source,
            name,
            fmt_odyssey::ResType::from_extension("ltr")?,
        )?;
        fmt_odyssey::ltr::Ltr::parse(&bytes).ok()
    };
    let names = (|| {
        Some(NameTables {
            male: ltr("humanm")?,
            female: ltr("humanf")?,
            last: ltr("humanl")?,
        })
    })();
    // Every `classes.2da` row, not only those creation offers: levelling up
    // shows a companion's own class (a Jedi in K1, a droid).
    let rows = assets.rows("classes");
    ChargenView {
        offer: ui::screens::chargen::offer_of(assets.game),
        classes: (0..rows).map(|row| class(assets, &classes, row)).collect(),
        genders: GENDERS.map(|strref| assets.text(Source::Campaign, strref).unwrap_or(Vec::new())),
        portraits,
        names,
        seed: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64),
        skill_text: std::array::from_fn(|skill| {
            number(assets.cell("skills", skill, "description")).unwrap_or(0)
        }),
        whole_skill_points: classes.whole_skill_points,
    }
}

/// Character creation's choices, read once it opens.
fn choices(mut ui: ResMut<Ui>) {
    let ui = &mut *ui;
    let creating = ui
        .stack
        .screens()
        .any(|screen| matches!(screen.0, "chargen" | "levelup"));
    if ui.vm.chargen.is_some() || !creating {
        return;
    }
    if let Some(assets) = ui.assets.as_mut() {
        ui.vm.chargen = Some(chargen(assets));
    }
}

/// The screens that show the character, the map, the journal or the
/// upgrade bench. The action bar setup shows the character's bar.
const PAGES: [&str; 14] = [
    "journal",
    "levelup",
    "equip",
    "inventory",
    "character",
    "abilities",
    ActionBar::NAME,
    "map",
    "options_in_game",
    "messages",
    "upgrade_select",
    "upgrade",
    "workbench",
    "chemical",
];

/// Row `row` of `table` as the pages list it: its name, icon and
/// description, the rulebook's lines for it after (`rulebook_text`).
fn entry(assets: &mut GuiAssets, table: &str, row: usize, columns: [&str; 3]) -> Entry {
    let [name, icon, description] = columns;
    let told = described_row(assets, table, row, description);
    Entry {
        name: assets
            .cell_text(table, row, name)
            .unwrap_or_else(|| format!("{table} {row}").into_bytes()),
        icon: assets.cell(table, row, icon),
        count: None,
        description: told,
        kind: None,
    }
}

/// Row `row`'s description in column `column` of `table`, followed by the
/// rulebook's lines for it (`rulebook_text`). The character pages and the
/// action bar setup both show this text.
pub(crate) fn described_row(
    assets: &mut GuiAssets,
    table: &str,
    row: usize,
    column: &str,
) -> Vec<u8> {
    let mut told = assets.cell_text(table, row, column).unwrap_or(Vec::new());
    let tags = rulebook_text::row_tags(assets, table, row);
    told.extend(rulebook_text::told(&tags));
    told
}

/// The rows of a known-rows set (`Rows`) below `limit`.
/// The `feat.2da` rows KOTOR 2's feat list never shows, though a character
/// has them: Complex Unarmed Animations (207) and Mine Immunity (238). TSL's
/// executable leaves these two rows out by number when it builds the list.
/// KOTOR 1's table has no such rows.
const TSL_UNLISTED_FEATS: [usize; 2] = [207, 238];

/// The `feat.2da` rows the character screen lists for a character that
/// `has` them, in KOTOR 2 (`tsl`) without `TSL_UNLISTED_FEATS`.
pub fn listed_feats(has: impl Fn(u16) -> bool, tsl: bool) -> impl Iterator<Item = usize> {
    known(has, sim::combat::sheet::Feats::ROWS)
        .filter(move |row| !(tsl && TSL_UNLISTED_FEATS.contains(row)))
}

/// The probes of the feat list (`kjk sim probes`, group "feat list").
#[cfg(feature = "probes")]
pub fn feat_list_probes() -> sim::probe::Checks {
    let all = |tsl| listed_feats(|row| matches!(row, 5 | 207 | 212 | 238), tsl).collect::<Vec<_>>();
    vec![
        (
            "tsl hides complex unarmed animations and mine immunity",
            sim::probe::check(all(true) == [5, 212], format!("{:?}", all(true))),
        ),
        (
            "k1 lists every feat it has",
            sim::probe::check(
                all(false) == [5, 207, 212, 238],
                format!("{:?}", all(false)),
            ),
        ),
    ]
}

fn known(rows: impl Fn(u16) -> bool, limit: u16) -> impl Iterator<Item = usize> {
    (0..limit).filter(move |row| rows(*row)).map(usize::from)
}

/// What an item `look` says of itself: TSL's "Cannot Equip: Bao-Dur" lines
/// first once he has been met (`bao_dur_met`, `sim::cannot_equip`), then
/// its stats in force (`ItemLook::in_force`: with `item`'s upgrades fitted,
/// K1's by its bits, TSL's by the upgrade items in its slots, whose own
/// stats follow), then its own description, then the rulebook's lines for
/// it (`rulebook_text`).
pub(crate) fn described(
    assets: &mut GuiAssets,
    look: &ItemLook,
    item: Option<&Item>,
    bao_dur_met: bool,
) -> Vec<u8> {
    let parts = item_text(assets, look, item, bao_dur_met);
    let fitted: Vec<ItemLook> = item
        .into_iter()
        .flat_map(|item| &item.fitted)
        .filter(|template| !template.is_empty())
        .filter_map(|template| assets.item(template))
        .collect();
    let edges: Vec<ui::assets::Edge> = fitted
        .iter()
        .flat_map(|upgrade| upgrade.edges.iter().cloned())
        .collect();
    let mut text = parts.written(
        parts.fitted,
        fitted.iter().flat_map(|upgrade| upgrade.in_force(0)),
        &edges,
    );
    let tags = rulebook_text::item_tags(assets, look);
    text.extend(rulebook_text::told(&tags));
    text
}

/// What an item `look` says of itself, in parts (`ItemText`): TSL's
/// "Cannot Equip: Bao-Dur" lines once he has been met, its stats with
/// `item`'s upgrade bits, and its own text.
fn item_text(
    assets: &mut GuiAssets,
    look: &ItemLook,
    item: Option<&Item>,
    bao_dur_met: bool,
) -> ItemText {
    let line = |strref| assets.text(Source::Campaign, strref).unwrap_or(Vec::new());
    ItemText {
        head: sim::cannot_equip::lines(look.base_item, bao_dur_met, line),
        stats: look.stats.clone(),
        fitted: look.upgrades | item.map_or(0, |item| item.upgrades),
        critical: look
            .critical
            .clone()
            .map(|critical| (critical, look.edges.clone())),
        text: look.description.clone(),
    }
}

/// Both games' "No Description Set": what the upgrade bench says of an
/// item with no text of its own.
const NO_DESCRIPTION: u32 = 32172;

/// `item_text` as the upgrade bench tells it (KOTOR 2's item property
/// description): KOTOR's "No Description Set" for an item with no text,
/// and none of the rulebook's lines.
fn bench_text(
    assets: &mut GuiAssets,
    look: &ItemLook,
    item: Option<&Item>,
    bao_dur_met: bool,
) -> ItemText {
    let mut parts = item_text(assets, look, item, bao_dur_met);
    if parts.text.is_empty() {
        parts.text = assets
            .text(Source::Campaign, NO_DESCRIPTION)
            .unwrap_or(Vec::new());
    }
    parts
}

/// Item `id` as the screens list it (no count for one that never runs
/// out), with the slots it may be worn in; `bao_dur_met` as `described`.
pub(crate) fn owned(assets: &mut GuiAssets, id: u32, item: &Item, bao_dur_met: bool) -> Owned {
    let look = assets.item(&item.template);
    let description = look.as_ref().map_or(Vec::new(), |look| {
        described(assets, look, Some(item), bao_dur_met)
    });
    Owned {
        item: id,
        slots: look.as_ref().map_or(0, |look| look.slots),
        usable: false,
        unfit: false,
        refused: None,
        unpaired: [false; 2],
        previews: Vec::new(),
        entry: Entry {
            name: look.as_ref().map_or_else(
                || item.template.as_bytes().to_vec(),
                |look| look.name.clone(),
            ),
            icon: look.as_ref().map(|look| look.icon.clone()),
            count: (!item.infinite).then_some(u32::from(item.stack)),
            kind: look.as_ref().map(|look| look.kind),
            description,
        },
    }
}

/// Both games' "Feat Required".
const FEAT_REQUIRED: u32 = 1055;

/// Why `wearer` may not wear `facts` (`State::equip_refusal`), as a line:
/// "Feat Required: " and the feats it lacks, by name, when those are why;
/// else the line the refusal shows (an alignment, class or race limit).
fn refusal(assets: &mut GuiAssets, facts: &ItemFacts, wearer: &Wearer) -> Option<Vec<u8>> {
    let notice = sim::script::State::equip_refusal(facts, wearer)?;
    let line = |strref| assets.text(Source::Campaign, strref).unwrap_or(Vec::new());
    let lacking: Vec<u16> = facts
        .needs
        .iter()
        .copied()
        .filter(|need| !sim::combat::passive::may_equip(&wearer.feats, &[*need]))
        .collect();
    if lacking.is_empty() {
        return Some(match notice {
            sim::notice::Notice::Line(strref) => line(strref),
            _ => line(sim::combat::passive::NOT_PROFICIENT),
        });
    }
    let mut words = line(FEAT_REQUIRED);
    words.extend_from_slice(b": ");
    let names: Vec<Vec<u8>> = lacking
        .into_iter()
        .filter_map(|feat| assets.cell_text("feat", usize::from(feat), "name"))
        .collect();
    words.extend(names.join(&b", "[..]));
    Some(words)
}

/// Base item `row` as a weapon the hands pair (`sim::inventory::pairs`):
/// how it delivers and how it is held (`baseitems.2da`); `None` for one
/// combat does not know as a weapon.
fn paired_as(assets: &mut GuiAssets, row: u16) -> Option<sim::combat::sheet::Weapon> {
    use sim::combat::sheet::{Weapon, build};
    let delivery = delivery(assets, row)?;
    let at = usize::from(row);
    let mut int = |column| assets.cell("baseitems", at, column)?.parse::<i64>().ok();
    let (wield, size) = (int("weaponwield"), int("weaponsize"));
    Some(Weapon {
        base_item: row,
        delivery,
        build: build::of(wield, size),
        ..Weapon::UNARMED
    })
}

/// How base item `row` delivers its hits (`Delivery::of_row`): a row the
/// games share by its number, any other (a mod's) by its columns. `None`
/// for a row that is no weapon.
pub(crate) fn delivery(assets: &mut GuiAssets, row: u16) -> Option<sim::combat::Delivery> {
    let at = usize::from(row);
    let columns = sim::combat::sheet::WeaponColumns::read(|column| {
        assets.cell("baseitems", at, column)?.parse::<i64>().ok()
    });
    sim::combat::Delivery::of_row(row, &columns)
}

/// The weapon the equip page shows numbers for in hand `hand` of `sheet`,
/// as combat fights with it (`damage::in_hand`). KOTOR 2's equip screen
/// shows the fists' numbers for an empty right hand. For an empty left
/// hand it shows the off-hand end of a double weapon in the right hand,
/// and nothing beside any other weapon.
fn shown_weapon(sheet: &Sheet, hand: usize) -> Option<sim::combat::Weapon> {
    let double = sheet.weapons[0]
        .is_some_and(|weapon| weapon.build & sim::combat::sheet::build::DOUBLE != 0);
    (hand == 0 || sheet.weapons[hand].is_some() || double)
        .then(|| sim::combat::damage::in_hand(sheet, hand as u8))
}

/// The damage one hit with hand `hand`'s weapon deals on `sheet`, from
/// its least to its most (`damage::damage_range`), as KOTOR 2's equip
/// screen writes it ("3-8"). It is empty when the hand shows no weapon.
fn damage(sheet: &Sheet, hand: usize) -> String {
    shown_weapon(sheet, hand).map_or(String::new(), |weapon| {
        let (least, most) = sim::combat::damage::damage_range(sheet, &weapon);
        format!("{least}-{most}")
    })
}

/// Hand `hand`'s attack modifier on `sheet` (`damage::attack_bonus`), as
/// the equip page shows it. It is `None` when the hand shows no weapon. It
/// counts the attack bonus of KOTOR 2's Form in use against an enemy who
/// holds no lightsaber (`forms::to_hit`): Shii-Cho's, Shien's and Niman's,
/// but not Makashi's, which counts only against a lightsaber.
fn attack(sheet: &Sheet, hand: usize) -> Option<i32> {
    let form = sim::combat::forms::to_hit(sheet, &Sheet::commoner());
    shown_weapon(sheet, hand).map(|weapon| sim::combat::damage::attack_bonus(sheet, &weapon) + form)
}

/// The slots the equip page previews an item in: the hands and the body.
const PREVIEWED: [u8; 3] = [
    sim::inventory::slots::RIGHT_WEAPON,
    sim::inventory::slots::LEFT_WEAPON,
    sim::inventory::slots::BODY,
];

/// `me`'s numbers on `sheet` were `item` put on in each previewed slot it
/// may go in, as the sim dresses a sheet (`inventory::rewear`, each item's
/// facts with its upgrades fitted).
fn previews(sheet: &Sheet, goods: &Goods, me: u32, item: &Item, catalog: &Catalog) -> Vec<Preview> {
    let facts = |item: &Item| {
        goods
            .facts(catalog, &item.template)
            .map(|facts| facts.fitted(item.upgrades, &item.fitted, catalog, &goods.known))
    };
    let Some(candidate) = facts(item) else {
        return Vec::new();
    };
    let worn: Vec<(u8, ItemFacts)> = goods
        .items
        .iter()
        .map(|(_, held)| held)
        .filter(|held| held.holder == me)
        .filter_map(|held| Some((held.slot?, facts(held)?)))
        .collect();
    let before: Vec<(u8, &ItemFacts)> = worn.iter().map(|(slot, facts)| (*slot, facts)).collect();
    PREVIEWED
        .into_iter()
        .filter(|slot| candidate.slots & 1 << slot != 0)
        .map(|slot| {
            let after: Vec<(u8, &ItemFacts)> = before
                .iter()
                .copied()
                .filter(|(at, _)| *at != slot)
                .chain([(slot, &candidate)])
                .collect();
            let dressed = sim::inventory::rewear(*sheet, &before, &after);
            Preview {
                slot,
                attack: [0, 1].map(|hand| attack(&dressed, hand)),
                damage: [0, 1].map(|hand| damage(&dressed, hand)),
                defence: dressed.defence(),
            }
        })
        .collect()
}

/// The character screens' view of a member whose own sheet is `own` and
/// whose sheet as its effects and worn items leave it is `sheet`.
fn view(
    assets: &mut GuiAssets,
    (own, sheet): (&Sheet, &Sheet),
    (hp, fp): (i32, i32),
    rules: sim::combat::saber::SaberRules,
    goods: &Goods,
    (me, bao_dur_met, library, wearer): (u32, bool, Option<&Library>, Option<&Wearer>),
) -> CharacterView {
    let catalog = library.map(|library| library.items());
    let saves = [0, 1, 2].map(|index| sheet.save(index));
    // As scripts read them (`GetSkillRank`), the totals both games' skill
    // screens show.
    let tsl = wearer.is_some_and(|wearer| wearer.tsl);
    let skill_totals: [i32; 8] =
        std::array::from_fn(|row| sim::stealth::rank(library, own, sheet, row, tsl));
    let weapon = |assets: &mut GuiAssets, hand: usize| {
        let weapon = sheet.weapons[hand].as_ref()?;
        let row = usize::from(weapon.base_item);
        let class = assets
            .cell("baseitems", row, "itemclass")
            .unwrap_or(String::new());
        Some(Entry {
            name: assets
                .cell_text("baseitems", row, "name")
                .unwrap_or(Vec::new()),
            icon: Some(format!("i{class}_001")),
            count: None,
            description: Vec::new(),
            kind: Some(ItemKind::Weapon),
        })
    };
    let skills = (0..sheet.skills.len())
        .map(|row| Entry {
            count: Some(skill_totals[row].max(0) as u32),
            ..entry(assets, "skills", row, ["name", "icon", "description"])
        })
        .collect();
    let powers = known(
        |row| sheet.powers.has(row),
        sim::combat::sheet::Powers::ROWS,
    )
    .map(|row| entry(assets, "spells", row, ["name", "iconresref", "spelldesc"]))
    .collect();
    let feats = listed_feats(|row| sheet.feats.has(row), tsl)
        .map(|row| entry(assets, "feat", row, ["name", "icon", "description"]))
        .collect();
    let weapon_of = |assets: &mut GuiAssets, item: &Item| {
        let row = assets.item_ref(&item.template)?.base_item;
        paired_as(assets, row)
    };
    // What each set's main hand holds, for the off hands' lists.
    let mains = sim::inventory::slots::SETS.map(|[right, _]| {
        goods
            .items
            .iter()
            .find(|(_, item)| item.holder == me && item.slot == Some(right))
            .and_then(|(_, item)| weapon_of(assets, item))
    });
    let carried: Vec<Owned> = goods
        .carried(PARTY)
        .map(|(id, item)| {
            let off = weapon_of(assets, item);
            // As the sim weighs an equip (`State::wear_with`): the item's
            // facts with its upgrades fitted.
            let facts = catalog.and_then(|catalog| {
                let facts = goods.facts(catalog, &item.template)?;
                Some(facts.fitted(item.upgrades, &item.fitted, catalog, &goods.known))
            });
            let rules = facts.as_ref().zip(wearer);
            Owned {
                unfit: rules.is_some_and(|(facts, wearer)| !facts.fits(wearer.race)),
                refused: rules.and_then(|(facts, wearer)| refusal(assets, facts, wearer)),
                previews: catalog.map_or(Vec::new(), |catalog| {
                    previews(sheet, goods, me, item, catalog)
                }),
                unpaired: mains.map(|main| {
                    !main
                        .zip(off)
                        .is_some_and(|(main, off)| sim::inventory::pairs(&main, &off))
                }),
                ..owned(assets, id, item, bao_dur_met)
            }
        })
        .collect();
    let worn = goods
        .items
        .iter()
        .filter(|(_, item)| item.holder == me)
        .filter_map(|(id, item)| Some((item.slot?, owned(assets, *id, item, bao_dur_met))))
        .collect();
    CharacterView {
        id: me,
        headgear_setting: None,
        stance_setting: None,
        stance: None,
        class_row: None,
        name: Vec::new(),
        classes: Vec::new(),
        portrait: None,
        level: sheet.level,
        hp: (hp / 100, i32::from(sheet.max_hp)),
        fp: force_bar(rules, sheet, fp),
        defence: sheet.defence(),
        abilities: sheet.abilities,
        own_abilities: own.abilities,
        own_skills: own.skills,
        skill_totals,
        saves,
        alignment: sheet.good_evil,
        weapons: [weapon(assets, 0), weapon(assets, 1)],
        damage: [0, 1].map(|hand| damage(sheet, hand)),
        attack: [0, 1].map(|hand| attack(sheet, hand)),
        skills,
        powers,
        feats,
        owned: carried,
        worn,
        locked: wearer.map_or(0, |wearer| wearer.locked),
        droid: sheet.race == sim::combat::sheet::races::DROID,
        credits: goods.gold,
        xp: 0,
        next_xp: None,
        may_level: false,
        next_level: None,
        auto_level: None,
        unspent: 0,
    }
}

/// The journal's quests, the latest updated first: each quest's name and
/// the text of the entry it reached (`global.jrl`'s, from the level's
/// library); a quest whose entry ends it counts 1. The player's tokens are
/// filled in as a name is shown (`dialogue_view::said`), so KOTOR 2's
/// "Rescue <FullName>" shows the player's name.
fn journal(
    assets: &mut GuiAssets,
    library: &sim::script::Library,
    progress: &sim::Progress,
    fill: crate::dialogue_view::Fill,
) -> Vec<QuestView> {
    let line = |assets: &mut GuiAssets, text: &sim::script::journal::JournalText| {
        let raw = text.words(|strref| assets.text(Source::Campaign, strref));
        crate::dialogue_view::said(&raw, fill)
    };
    progress
        .journal
        .iter()
        .rev()
        .filter_map(|(tag, reached)| {
            let quest = library.quests().get(tag)?;
            let entry = u32::try_from(*reached).ok().and_then(|id| quest.entry(id));
            // KOTOR 2 writes the quest's planet over its entry: "Peragus:".
            let mut description = usize::try_from(quest.planet)
                .ok()
                .and_then(|row| assets.cell_text(PLANETARY, row, "name"))
                .map(|planet| crate::dialogue_view::said(&planet, fill))
                .filter(|planet| !planet.is_empty())
                .map_or(Vec::new(), |mut planet| {
                    planet.extend_from_slice(b":\n");
                    planet
                });
            description.extend(entry.map_or(Vec::new(), |entry| line(assets, &entry.text)));
            Some(QuestView {
                name: line(assets, &quest.name),
                description,
                done: entry.is_some_and(|entry| entry.end),
                planet: quest.planet,
            })
        })
        .collect()
}

/// The planets' table, whose names the journal writes.
const PLANETARY: &str = "planetary";

/// A party member in the presented snapshot: its object id, sheet and
/// fighter, its class row when its template gives one (a companion's), and
/// how the bar and the HUD show it.
struct Member {
    id: u32,
    /// The setting that hides its headgear (`headgear::setting`).
    headgear_setting: Option<String>,
    /// The setting its own stance is kept in (`stance_sets::pick_setting`),
    /// when this seat may pick its stance (`stance_sets::may_pick`).
    stance_setting: Option<String>,
    /// The title of the stance set it stands in or waits for.
    stance: Option<String>,
    sheet: Sheet,
    fighter: sim::combat::Fighter,
    class_row: Option<u8>,
    career: sim::levels::Career,
    /// As an equip's rules weigh it (`sim::script::items::wearer`, the
    /// host's own).
    wearer: Wearer,
    view: MemberView,
}

/// The characters `listed` for `own` (`cast::party_of`, or the pages'
/// `cast::pages_of`): its avatar named as made, its companions by their
/// templates' names, first classes and portraits. The player dresses its
/// own body; the host also the companions no seat plays (I30 P14).
fn members(
    assets: &mut GuiAssets,
    snapshot: &sim::Snapshot,
    (own, listed): (sim::Seat, Vec<&sim::EntityState>),
    level: Option<&sim::Level>,
    made: &Made,
) -> Vec<Member> {
    let instances = level.map(|level| sim::Instances::in_snapshot(level, snapshot));
    let tsl = level
        .and_then(|level| level.module())
        .is_some_and(|module| module.game == "tsl");
    let class_name = |assets: &mut GuiAssets, row: usize| {
        assets
            .cell_text("classes", row, "name")
            .unwrap_or(Vec::new())
    };
    let mut party = Vec::new();
    for (at, entity) in listed.into_iter().enumerate() {
        let data = instances
            .as_ref()
            .and_then(|instances| instances.of_entity(entity));
        let (Some(sheet), Some(fighter)) = (
            entity.sheet.or(data.and_then(|data| data.sheet)),
            entity.fighter,
        ) else {
            continue;
        };
        // A player's: its persona and its career (character creation's).
        let persona = match entity.kind {
            sim::Kind::Avatar(seat) => sim::persona::of(&snapshot.personas, seat),
            _ => None,
        };
        let career = snapshot.progress.careers.get(&entity.id.0);
        let class_row = data
            .and_then(|data| data.classes.first())
            .or(career.and_then(|career| career.first()))
            .map(|(row, _)| *row);
        // As the sim's level-up reads it (`script::levelling::career`).
        let career = career.cloned().unwrap_or_else(|| {
            let template = data.map_or(&[][..], |data| &data.classes[..]);
            let made = made.0.as_ref().map_or(0, |made| made.class);
            sim::levels::untaken(template, sheet.level, made)
        });
        let named_class = |assets: &mut GuiAssets, row: Option<u8>| {
            row.map_or(Vec::new(), |row| class_name(assets, usize::from(row)))
        };
        // Its portrait darkens with its alignment (`alignment_look`).
        let step = crate::alignment_look::aligned(tsl, sheet.good_evil).step;
        let (name, class, portrait) = match (data, persona, &made.0) {
            (Some(data), ..) => (
                fmt_odyssey::code_page_bytes(&data.name),
                named_class(assets, class_row),
                None,
            ),
            (None, Some(persona), _) => (
                fmt_odyssey::code_page_bytes(&persona.name),
                named_class(assets, class_row),
                assets.portrait(usize::from(persona.portrait), step),
            ),
            (None, None, Some(made)) => (
                fmt_odyssey::code_page_bytes(&made.name),
                class_name(assets, usize::from(made.class)),
                Some(made.portrait.clone()),
            ),
            (None, None, None) => (Vec::new(), Vec::new(), None),
        };
        let portrait = portrait.or_else(|| assets.portrait_of(&data?.template, step));
        let dresses =
            at == 0 || (own == sim::Seat(0) && fighter.mind.driver.is_none_or(|seat| seat == own));
        let may_level = dresses
            && level
                .and_then(|level| level.scripts())
                .is_some_and(|library| {
                    library
                        .classes()
                        .may_level(sheet.level, snapshot.progress.xp)
                });
        // The seat playing it, for its portrait's icon (R10).
        let seat = (0..snapshot.seats()).map(sim::Seat).find(|seat| {
            snapshot
                .body(*seat)
                .is_some_and(|body| body.id == entity.id)
        });
        let wearer = Wearer {
            good_evil: sheet.good_evil,
            classes: [0, 1].map(|at| {
                data.map_or(&career[..], |data| &data.classes[..])
                    .get(at)
                    .map_or(u8::MAX, |(class, _)| *class)
            }),
            race: sheet.race,
            subrace: data.map_or(0, |data| data.subrace),
            feats: sheet.feats,
            gender: data
                .map(|data| data.gender)
                .or(persona.map(|persona| persona.gender))
                .unwrap_or(0),
            abilities: sheet.abilities,
            is_pc: seat.is_some(),
            tag: data.map_or_else(String::new, |data| data.tag.clone()),
            tsl,
            // As the sim weighs it (`script::items::wearer`): its body's row.
            locked: sim::script::body_row(fighter.look, data, persona)
                .zip(level.and_then(|level| level.scripts()))
                .map_or(0, |(row, library)| library.locked_slots(row)),
        };
        let who = crate::headgear::who(instances.as_ref(), entity);
        party.push(Member {
            id: entity.id.0,
            headgear_setting: who.as_deref().map(crate::headgear::setting),
            stance_setting: who
                .as_deref()
                .filter(|_| crate::stance_sets::may_pick(own, entity))
                .map(crate::stance_sets::pick_setting),
            stance: crate::stance_sets::standing_title(
                level.and_then(|level| level.anims().stances()),
                &fighter,
            ),
            sheet,
            fighter,
            class_row,
            career,
            wearer,
            view: MemberView {
                name,
                class,
                level: sheet.level,
                portrait,
                hp: (fighter.hp / 100, i32::from(sheet.max_hp)),
                fp: force_bar(snapshot.saber_rules, &sheet, fighter.fp),
                dresses,
                may_level,
                busy: seat.and_then(|seat| crate::hud::busy(snapshot, seat)),
                played_by: seat.filter(|seat| *seat != own).map(|seat| seat.0),
                effects: effect_icons(assets, &fighter, snapshot.time_ms),
                debilitated: fighter.debilitated(snapshot.time_ms),
                poisoned: fighter.pack.poison.is_some(),
                id: entity.id.0,
            },
        });
    }
    party
}

/// The Force bar of a character's screens: its points and its most, as the
/// older second bar counts them. Under the Advanced rules only a Force user
/// has one, because Stamina is the bar every other character has.
fn force_bar(rules: sim::combat::saber::SaberRules, sheet: &Sheet, fp: i32) -> (i32, i32) {
    if rules.duel() && sheet.max_fp == 0 {
        (0, 0)
    } else {
        (fp / 100, i32::from(sheet.pool()))
    }
}

/// `fighter`'s running effect icons as KOTOR counts them on its portrait
/// (`CSWSEffectListHandler::OnApplyEffectIcon`): each its `effecticon.2da`
/// row's picture, good or bad by the row's `good`; an icon whose row has
/// no `good` (the alignment's full marks) or no picture left out.
fn effect_icons(
    assets: &mut GuiAssets,
    fighter: &sim::combat::Fighter,
    now: u64,
) -> Vec<EffectIcon> {
    fighter
        .effects
        .icons(now)
        .filter_map(|row| {
            let row = usize::from(row);
            let good = assets.cell("effecticon", row, "good")? != "0";
            let icon = assets.cell("effecticon", row, "iconresref")?;
            Some(EffectIcon { icon, good })
        })
        .collect()
}

/// What an upgrade bench lists: what the party carries and what its
/// members wear that takes upgrades (`ItemFacts::upgrades`) or a colour
/// crystal (a lightsaber), with its slots (`upgrade_slots`).
fn upgrades(
    assets: &mut GuiAssets,
    goods: &Goods,
    party: &[Member],
    (catalog, bao_dur_met): (Option<&Catalog>, bool),
) -> Vec<UpgradeItem> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    let holders: Vec<u32> = [PARTY]
        .into_iter()
        .chain(party.iter().map(|member| member.id))
        .collect();
    goods
        .items
        .iter()
        .filter(|(_, item)| holders.contains(&item.holder))
        .filter_map(|(id, item)| {
            let kind = assets.item_ref(&item.template)?.upgrade?;
            let slots = upgrade_slots(assets, goods, catalog, item)?;
            let look = assets.item(&item.template)?;
            Some(UpgradeItem {
                id: *id,
                item: owned(assets, *id, item, bao_dur_met).entry,
                template: item.template.clone(),
                kind,
                slots,
                text: bench_text(assets, &look, Some(item), bao_dur_met),
            })
        })
        .collect()
}

/// An upgrade's template as the bench shows it (no upgrade is an item
/// Bao-Dur cannot equip): its row in a list, its whole description as the
/// bench tells it, and its own stats. `row` is its `upgrade.2da` row where
/// fitting it sets the item's bit (KOTOR 1's).
fn part(assets: &mut GuiAssets, template: &str, row: Option<u8>) -> UpgradePart {
    let item = Item {
        template: template.to_string(),
        holder: PARTY,
        slot: None,
        stack: 1,
        charges: 0,
        droppable: true,
        plot: false,
        identified: true,
        infinite: true,
        non_equippable: false,
        upgrades: 0,
        fitted: Vec::new(),
    };
    let entry = owned(assets, 0, &item, false).entry;
    let Some(look) = assets.item(template) else {
        return UpgradePart {
            entry,
            ..UpgradePart::default()
        };
    };
    let text = bench_text(assets, &look, None, false);
    UpgradePart {
        entry: Entry {
            description: text.written(text.fitted, std::iter::empty(), &[]),
            ..entry
        },
        stats: text
            .stats_in_force(text.fitted)
            .map(<[u8]>::to_vec)
            .collect(),
        edges: look.edges.clone(),
        row,
    }
}

/// The name and the empty slot's picture of each kind of upgrade slot, as
/// each game's executable keeps them for its upgrade screen: by
/// `upgradetypes.2da` row, the colour crystal's slot as `COLOUR_SLOT`.
const TSL_SLOTS: [(u8, &str, u32); 13] = [
    (sim::inventory::COLOUR_SLOT, "i_powerc", 48472),
    (0, "i_powerc", 48473),
    (1, "i_lens", 48464),
    (2, "i_emitter", 48465),
    (3, "i_lsenergy", 48466),
    (4, "i_scope", 48471),
    (5, "i_energy", 48470),
    (6, "i_imp_eng", 48469),
    (7, "i_grip", 48467),
    (8, "i_edge", 48468),
    (9, "i_vcell", 48466),
    (10, "i_armorrein", 48475),
    (11, "i_armorrein", 48474),
];
const K1_SLOTS: [(u8, &str, u32); 11] = [
    (sim::inventory::COLOUR_SLOT, "i_colorc", 36978),
    (0, "i_powerc", 36977),
    (1, "i_vcell", 32493),
    (2, "i_durasteel", 32494),
    (3, "i_imp_eng", 32495),
    (4, "i_scope", 32487),
    (5, "i_energy", 32488),
    (6, "i_beam", 32489),
    (7, "i_hair", 32490),
    (8, "i_armorrein", 32491),
    (9, "i_armorrein", 32492),
];

/// A slot of kind `kind` (TSL's when `tsl`), with nothing yet: its name
/// and its empty picture.
fn empty_slot(assets: &mut GuiAssets, tsl: bool, kind: u8) -> UpgradeSlot {
    let table: &[(u8, &str, u32)] = if tsl { &TSL_SLOTS } else { &K1_SLOTS };
    let Some(&(_, icon, strref)) = table.iter().find(|(of, ..)| *of == kind) else {
        return UpgradeSlot::default();
    };
    UpgradeSlot {
        name: assets.text(Source::Campaign, strref).unwrap_or(Vec::new()),
        icon: Some(icon.to_string()),
        ..UpgradeSlot::default()
    }
}

/// `item`'s slots (none when it takes no upgrade): a lightsaber's colour
/// crystal first (its own, which stays; the colour crystals the party
/// carries that make a saber of its hilt), then each kind of upgrade it
/// takes (`upgrade.2da` `upgradetype`), as many slots as the kind holds
/// (`properties::room`): what is fitted, and the upgrades the party
/// carries of rows it takes and has not fitted. TSL's slots list every
/// upgrade of their kind the party carries, those above the item's upgrade
/// level (but crystals) as not taken.
fn upgrade_slots(
    assets: &mut GuiAssets,
    goods: &Goods,
    catalog: &Catalog,
    item: &Item,
) -> Option<Vec<UpgradeSlot>> {
    let facts = goods.facts(catalog, &item.template)?;
    let carried: Vec<(u32, &sim::inventory::ItemFacts)> = goods
        .carried(PARTY)
        .filter_map(|(id, held)| Some((id, goods.facts(catalog, &held.template)?)))
        .collect();
    let mut slots = Vec::new();
    let recolours =
        |crystal: &sim::inventory::ItemFacts| catalog.recolour(crystal, facts.base_item).is_some();
    if !facts.slot_kinds.is_empty() {
        // TSL's: each slot of its kind, by its index (the colour crystal's
        // as K1's), the upgrades of its kind and of no higher level the
        // party carries.
        for (at, kind) in facts.slot_kinds.iter().enumerate() {
            if *kind == sim::inventory::COLOUR_SLOT {
                slots.push(colour_slot(
                    assets,
                    catalog,
                    &carried,
                    item,
                    (true, recolours),
                ));
                continue;
            }
            let fitted = item
                .fitted
                .get(at)
                .filter(|template| !template.is_empty())
                .map(|template| (Some(at as u8), part(assets, template, None)));
            let choices = carried
                .iter()
                .filter(|(_, upgrade)| upgrade.upgrade.is_some_and(|(_, of)| of == *kind))
                .map(|(id, upgrade)| UpgradeChoice {
                    id: *id,
                    part: part(assets, &upgrade.template, None),
                    fits: upgrade.upgrade_level <= facts.upgrade_level
                        || *kind == sim::inventory::CRYSTAL,
                })
                .collect();
            slots.push(UpgradeSlot {
                fitted,
                choices,
                at: Some(at as u8),
                ..empty_slot(assets, true, *kind)
            });
        }
        return (!slots.is_empty()).then_some(slots);
    }
    if catalog.iter().any(recolours) {
        slots.push(colour_slot(
            assets,
            catalog,
            &carried,
            item,
            (false, recolours),
        ));
    }
    let mut kinds: Vec<u8> = facts.upgrades.iter().map(|upgrade| upgrade.kind).collect();
    kinds.sort();
    kinds.dedup();
    for kind in kinds {
        let of_kind: Vec<&sim::inventory::Upgrade> = facts
            .upgrades
            .iter()
            .filter(|upgrade| upgrade.kind == kind)
            .collect();
        let fitted: Vec<&sim::inventory::Upgrade> = of_kind
            .iter()
            .copied()
            .filter(|upgrade| has_upgrade(item.upgrades, upgrade.row))
            .collect();
        let choices: Vec<UpgradeChoice> = carried
            .iter()
            .filter(|(_, part)| {
                part.upgrade.is_some_and(|(row, _)| {
                    of_kind.iter().any(|upgrade| u16::from(upgrade.row) == row)
                        && u8::try_from(row).is_ok_and(|row| !has_upgrade(item.upgrades, row))
                })
            })
            .map(|(id, upgrade)| fitting(assets, *id, upgrade, true))
            .collect();
        // The rows of the kind not fitted, one to each empty slot in turn:
        // the item's own stats an upgrade put in there would bring.
        let free: Vec<u8> = of_kind
            .iter()
            .map(|upgrade| upgrade.row)
            .filter(|row| !has_upgrade(item.upgrades, *row))
            .collect();
        for at in 0..sim::inventory::properties::room(kind) {
            slots.push(UpgradeSlot {
                at: None,
                fitted: fitted.get(at).map(|upgrade| {
                    let shown = part(assets, &upgrade.template, Some(upgrade.row));
                    (Some(upgrade.row), shown)
                }),
                choices: choices.clone(),
                keyed: at
                    .checked_sub(fitted.len())
                    .and_then(|free_at| free.get(free_at).copied()),
                ..empty_slot(assets, false, kind)
            });
        }
    }
    (!slots.is_empty()).then_some(slots)
}

/// An upgrade the party carries that the item takes, by its object id;
/// with its `upgrade.2da` row as the item's bit when `keyed` (KOTOR 1's).
fn fitting(
    assets: &mut GuiAssets,
    id: u32,
    upgrade: &sim::inventory::ItemFacts,
    keyed: bool,
) -> UpgradeChoice {
    let row = upgrade
        .upgrade
        .filter(|_| keyed)
        .and_then(|(row, _)| u8::try_from(row).ok());
    UpgradeChoice {
        id,
        part: part(assets, &upgrade.template, row),
        fits: true,
    }
}

/// A lightsaber's colour crystal slot (TSL's when `tsl`): its own crystal,
/// which stays; the colour crystals the party carries that make a saber of
/// its hilt.
fn colour_slot(
    assets: &mut GuiAssets,
    catalog: &Catalog,
    carried: &[(u32, &sim::inventory::ItemFacts)],
    item: &Item,
    (tsl, recolours): (bool, impl Fn(&sim::inventory::ItemFacts) -> bool),
) -> UpgradeSlot {
    let own = catalog.crystal_of(&item.template);
    let choices = carried
        .iter()
        .filter(|(_, crystal)| recolours(crystal) && !crystal.recolours.contains(&item.template))
        .map(|(id, crystal)| fitting(assets, *id, crystal, false))
        .collect();
    UpgradeSlot {
        fitted: own.map(|crystal| (None, part(assets, &crystal.template, None))),
        choices,
        at: None,
        ..empty_slot(assets, tsl, sim::inventory::COLOUR_SLOT)
    }
}

fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    made: Res<Made>,
    (shown, bench): (
        Res<crate::script_screens::Shown>,
        Res<crate::workbench_view::OpenBench>,
    ),
    scene: Option<Res<crate::scene::SceneContent>>,
    mut ui: ResMut<Ui>,
) {
    let ui = &mut *ui;
    let (Some(presented), Some(own), Some(assets)) = (presented, own, ui.assets.as_mut()) else {
        return;
    };
    let snapshot = presented.0.current();
    let level = swap.as_ref().map(|swap| &swap.level);
    let open = !shown.0.is_empty() || ui.stack.screens().any(|screen| PAGES.contains(&screen.0));
    // The HUD shows the party alone. The pages also show the companions
    // waiting at a base (`cast::pages_of`).
    let listed = if open {
        crate::cast::pages_of(snapshot, own.0)
    } else {
        crate::cast::bar_of(snapshot, own.0)
    };
    let party = members(assets, snapshot, (own.0, listed), level, &made);
    ui.vm.party = party.iter().map(|member| member.view.clone()).collect();
    if !open || ui.vm.viewed >= party.len() {
        ui.vm.viewed = 0;
    }
    if !open {
        return;
    }
    let library_now = swap
        .as_ref()
        .and_then(|swap| swap.level.scripts())
        .map(|library| &**library);
    let catalog = library_now.map(|library| library.items());
    let bao_dur_met = snapshot.screens.bao_dur_met;
    ui.vm.upgrades = upgrades(assets, &snapshot.goods, &party, (catalog, bao_dur_met));
    ui.vm.workbench = bench.0.as_ref().map(|(bench, shop)| {
        crate::workbench_view::view(assets, (bench, shop), snapshot, library_now)
    });
    let viewed = ui.vm.viewed;
    ui.vm.character = party.get(viewed).map(|member| CharacterView {
        name: member.view.name.clone(),
        classes: member
            .career
            .iter()
            .map(|&(class, levels)| {
                let name = assets.cell_text("classes", usize::from(class), "name");
                (name.unwrap_or(Vec::new()), levels)
            })
            .collect(),
        portrait: member.view.portrait.clone(),
        class_row: member.class_row,
        headgear_setting: member.headgear_setting.clone(),
        stance_setting: member.stance_setting.clone(),
        stance: member.stance.clone(),
        // Its own sheet, the one the host levels from, and the sheet as
        // effects and worn items leave it (KOTOR's screen shows a belt's
        // strength in the score).
        ..view(
            assets,
            (
                &member.sheet,
                &sim::combat::effects::sheet_now(member.sheet, &member.fighter, snapshot.time_ms),
            ),
            (member.fighter.hp, member.fighter.fp),
            snapshot.saber_rules,
            &snapshot.goods,
            (member.id, bao_dur_met, library_now, Some(&member.wearer)),
        )
    });
    let library = swap.as_ref().and_then(|swap| swap.level.scripts().cloned());
    let progress = &snapshot.progress;
    if let (Some(me), Some(library)) = (ui.vm.character.as_mut(), &library) {
        // What the member shown may use on itself: an item carried that
        // is worn by none, whose spell is no hostile one or one its table
        // lists for its user's own use, for its race
        // (`sim::script::items::spell_for`, the host's own rule).
        let race = party.get(viewed).map(|member| member.sheet.race);
        for owned in &mut me.owned {
            owned.usable = race
                .zip(
                    snapshot
                        .goods
                        .items
                        .iter()
                        .find(|(id, _)| *id == owned.item),
                )
                .and_then(|(race, (_, item))| {
                    let facts = snapshot.goods.facts(library.items(), &item.template)?;
                    (facts.slots == 0)
                        .then(|| sim::script::items::spell_for(library, facts, race, true))
                        .flatten()
                })
                .is_some();
        }
        let classes = library.classes();
        me.xp = progress.xp;
        me.next_xp = classes.next(me.level);
        me.may_level = party
            .get(viewed)
            .is_some_and(|member| member.view.may_level);
        me.unspent = party.get(viewed).map_or(0, |member| {
            progress.unspent.get(&member.id).copied().unwrap_or(0)
        });
        // The next level is in its latest class (a Jedi's once it is one).
        let next = party
            .get(viewed)
            .filter(|_| me.may_level)
            .and_then(|member| {
                let career = &member.career;
                let &(class, _) = career.last()?;
                let picks = classes.picks(&member.sheet, career, class)?;
                let auto = classes.auto(&member.sheet, career, class, me.unspent)?;
                Some((class, picks_view(assets, &picks), auto))
            });
        me.auto_level = next.as_ref().map(|(_, _, auto)| AutoLevelView {
            class: auto.class,
            ability: auto.ability,
            skills: auto.skills,
            feats: auto.feats.clone(),
            powers: auto.powers.clone(),
        });
        me.next_level = next.map(|(class, picks, _)| (class, picks));
    }
    // The level the member shown may take next, from its ranks now (a level
    // taken, or another member shown, moves it on).
    if let Some(me) = ui.vm.character.as_ref().filter(|me| me.may_level)
        && ui
            .vm
            .levelup
            .as_ref()
            .map(|up| (up.who, up.level, up.unspent))
            != Some((me.id, me.level.saturating_add(1), me.unspent))
    {
        ui.vm.levelup = Some(crate::script_screens::level_up(me, &made));
    }
    ui.vm.journal = library.map_or(Vec::new(), |library| {
        journal(
            assets,
            &library,
            progress,
            crate::dialogue_view::Fill::party(snapshot),
        )
    });
    let map = scene.as_ref().and_then(|scene| scene.0.map.as_ref());
    ui.vm.map =
        swap.and_then(|swap| crate::map_view::page(assets, map, &swap.level, snapshot, own.0));
}

/// The probes (`kjk sim probes --only "equip numbers"`): the equip page's
/// attack and damage lines count what combat counts. A character with
/// Strength 14 holding a 1d6 blade shows "+2" and "3-8". The same blade
/// with a 1d4 upgrade, a +1 damage property and 1d6 fire damage shows
/// "6-19". A character with Dexterity 16 holding a 1d8 pistol shows "+3"
/// and "1-8", because Strength adds nothing to a gun without Mighty. An
/// empty right hand shows the fists' numbers, and an empty left hand
/// shows nothing. A character with Strength 14 and Master Two-Weapon
/// Fighting holding a 1d6 double blade shows "+2" and "3-8" for the main
/// end and "0" and "3-8" for the off-hand end.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::combat::gear::{Extra, Roll};
    use sim::combat::sheet::{Ability, Blaster};
    use sim::combat::{Delivery, Weapon};
    let with = |ability: Ability, score: u8, weapon: Option<Weapon>| {
        let mut sheet = Sheet::commoner();
        sheet.abilities[ability as usize] = score;
        sheet.weapons = [weapon, None];
        sheet
    };
    let blade = Weapon {
        delivery: Delivery::Melee,
        dice: 1,
        die: 6,
        ..Weapon::UNARMED
    };
    let upgraded = Weapon {
        bonus_dice: 1,
        bonus_die: 4,
        damage_bonus: 1,
        extras: [
            Some(Extra {
                damage_type: 0,
                roll: Roll {
                    dice: 1,
                    die: 6,
                    bonus: 0,
                },
            }),
            None,
            None,
        ],
        ..blade
    };
    let pistol = Weapon {
        delivery: Delivery::Blaster(Blaster::Pistol),
        dice: 1,
        die: 8,
        ..Weapon::UNARMED
    };
    let double = Weapon {
        build: sim::combat::sheet::build::DOUBLE,
        ..blade
    };
    let lines = |sheet: &Sheet| [0, 1].map(|hand| (attack(sheet, hand), damage(sheet, hand)));
    let blade_lines = lines(&with(Ability::Strength, 14, Some(blade)))[0].clone();
    let upgraded_lines = lines(&with(Ability::Strength, 14, Some(upgraded)))[0].clone();
    let pistol_lines = lines(&with(Ability::Dexterity, 16, Some(pistol)))[0].clone();
    let empty = lines(&with(Ability::Strength, 14, None));
    let master_double = lines(&Sheet {
        feats: sim::combat::sheet::feats::TWO_WEAPON_FIGHTING
            .iter()
            .fold(Default::default(), |rows, feat| rows.with(*feat)),
        ..with(Ability::Strength, 14, Some(double))
    });
    vec![
        (
            "equip_numbers_count_strength",
            sim::probe::check(
                blade_lines == (Some(2), "3-8".to_string()),
                format!("Strength 14 with a 1d6 blade: {blade_lines:?}"),
            ),
        ),
        (
            "equip_numbers_count_upgrades",
            sim::probe::check(
                upgraded_lines == (Some(2), "6-19".to_string()),
                format!("the blade with 1d4, +1 and 1d6 fire: {upgraded_lines:?}"),
            ),
        ),
        (
            "equip_numbers_ranged_dexterity",
            sim::probe::check(
                pistol_lines == (Some(3), "1-8".to_string()),
                format!("Dexterity 16 with a 1d8 pistol: {pistol_lines:?}"),
            ),
        ),
        (
            "equip_numbers_empty_hands",
            sim::probe::check(
                empty[0].0 == Some(2)
                    && !empty[0].1.is_empty()
                    && empty[1] == (None, String::new()),
                format!("both hands empty: {empty:?}"),
            ),
        ),
        (
            "equip_numbers_double_weapon",
            sim::probe::check(
                master_double == [(Some(2), "3-8".to_string()), (Some(0), "3-8".to_string())],
                format!("Master Two-Weapon Fighting with a double blade: {master_double:?}"),
            ),
        ),
    ]
}
