//! The fight's HUD in `kjk play` (render side only: it reads the
//! presented snapshots and `sim_host::SelectedPower`), laid out in KOTOR's
//! 640 × 480 GUI space at the bottom left:
//!
//! - the player's hit points (green, turning red as they fall) and its
//!   second bar (blue for FP, gold for stamina), the energy shield's points
//!   left (cyan) while one is up; over them, while a duel mode is played
//!   and it holds a saber, its duel bars (`duel_view::bars`: block points
//!   under MDA, then stamina);
//! - the KOTOR icon (`spells.2da` `iconresref`; pull shows push's) of the
//!   power the action bar or a script used last, for a character that
//!   knows powers;
//! - the four kinds of items it carries, as slots coloured by kind (grenade
//!   orange, healing green, shield cyan, stim yellow), a pip a use left (at
//!   most eight).
//!
//! It is drawn over the scene by the camera that composites the screens
//! (`ui_view`), before them, so the frame pays for one overlay camera, not
//! two; without screens, by a camera of its own, as the loading screen is
//! (`lifecycle_view`), so it needs no UI plugin and shows in headless
//! captures. Where KOTOR's own HUD is drawn (`ui_view::KotorHud`) this one
//! stands down; it fills that one's view-model (`HudView`): the bars, the
//! portrait made in creation, the action bar of the character played
//! (`bar_view`), the duel bars, and the minimap (the area's picture, where the
//! player stands on it and which way it faces).

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::texture::TextureIr;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::prelude::*;
use sim::combat::force;
use sim::combat::items::{self, KINDS, Use};
use vfs::{Discovery, Game};

use crate::lifecycle_view::{gui_quad, unlit};
use crate::material::OdysseyMaterial;
use crate::pad::Prompts;
use crate::settings::Action;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SelectedPower};

/// The overlay camera's layer, shared with the conversation screen
/// (`dialogue_view`).
const LAYER: usize = 8;
/// The fight HUD's own parts, which the same camera draws unless KOTOR's
/// HUD stands in for them.
const PARTS: usize = 10;
/// The bars (left, top, width, height): hit points, the second bar, the
/// shield.
const HP: [f32; 4] = [16.0, 438.0, 150.0, 9.0];
const BAR: [f32; 4] = [16.0, 450.0, 150.0, 6.0];
const SHIELD: [f32; 4] = [16.0, 459.0, 150.0, 3.0];
/// The duel bars over them: block points, then stamina.
const DUEL: [[f32; 4]; 2] = [[16.0, 426.0, 150.0, 3.0], [16.0, 431.0, 150.0, 3.0]];
/// The power icon, and the first item slot (the rest follow to its right).
const POWER: [f32; 4] = [174.0, 430.0, 32.0, 32.0];
const SLOT: [f32; 4] = [214.0, 432.0, 20.0, 20.0];
const PIPS: usize = 8;

/// Each power row's icon (by `Command::force`: pull's is push's), and the
/// conversation's talk table and install (`dialogue_view`).
pub struct HudSetup {
    icons: BTreeMap<u16, TextureIr>,
    dialogue: crate::dialogue_view::Setup,
}

/// The icons of every `spells.2da` row the power key can select, read from
/// `game`'s install (a row whose icon does not read has none): each row the
/// sim maps to a power, and each row a menu of the game offers (its
/// `forcehostile` or `forcefriendly` cell, `script::Menu`). They are read
/// through `installer` (the stage's, open already), else one opened here.
pub fn load(
    discovery: &Discovery,
    game: Game,
    installer: Option<Arc<crate::lifecycle::Installer>>,
) -> HudSetup {
    let mut icons = BTreeMap::new();
    let dialogue = crate::dialogue_view::load(discovery, game, installer);
    let resolver = dialogue.base();
    let table = resolver.and_then(|resolver| resolver.two_da("spells"));
    if let (Some(resolver), Some(table)) = (resolver, &table) {
        for at in 0..table.cells.len() {
            let Ok(row) = u16::try_from(at) else {
                break;
            };
            let offered = ["forcehostile", "forcefriendly"]
                .iter()
                .any(|column| table.text(at, column).is_some());
            if row == force::PULL || (force::of_row(row).is_none() && !offered) {
                continue;
            }
            let icon = table
                .resref(at, "iconresref")
                .and_then(|name| crate::viewer::texture(resolver, &name));
            if let Some(icon) = icon {
                icons.insert(row, icon);
            }
        }
    }
    if let Some(push) = icons.get(&PUSH).cloned() {
        icons.insert(force::PULL, push);
    }
    HudSetup { icons, dialogue }
}

/// Its systems; a session comes in with `install`.
pub struct HudPlugin;

/// A session's HUD, put into `app`: it is built on the next frame.
pub fn install(app: &mut World, setup: HudSetup) {
    let HudSetup { icons, dialogue } = setup;
    app.insert_resource(Pending(Some(icons)));
    crate::dialogue_view::install(app, dialogue);
}

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::dialogue_view::DialogueViewPlugin)
            .init_resource::<crate::progress::Told>()
            .add_systems(PreUpdate, spawn.run_if(resource_exists::<Pending>))
            .add_systems(
                Update,
                (follow, show, crate::progress::marks.after(show))
                    .run_if(resource_exists::<Hud>)
                    .before(crate::ui_view::Stepped),
            );
    }
}

#[derive(Resource)]
struct Pending(Option<BTreeMap<u16, TextureIr>>);

/// The HUD's parts.
#[derive(Resource)]
struct Hud {
    /// Its camera, drawing into what the scene's camera draws into: the
    /// screens' (`ui_view::UiCamera`), else its own (`own`).
    camera: Entity,
    own: bool,
    hp: Part,
    bar: Part,
    shield: Part,
    /// Each duel bar's shade and bar.
    duel: [(Part, Part); 2],
    power: Entity,
    /// Each power row's material (its icon).
    icons: BTreeMap<u16, Handle<OdysseyMaterial>>,
    slots: [Part; KINDS],
    pips: [[Part; PIPS]; KINDS],
}

/// A quad, and its own material (its tint changes).
#[derive(Clone, Copy)]
struct Part {
    entity: Entity,
    material: AssetId<OdysseyMaterial>,
}

/// The view an overlay camera that draws the HUD looks through: KOTOR's
/// 480 points high, from in front of its quads.
pub(crate) fn overlay_view() -> impl Bundle {
    (
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 480.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 100.0),
    )
}

/// A camera of the HUD's own, for a run without screens.
fn own_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            crate::scene::over_world(11),
            overlay_view(),
            crate::scene::encoded_view(),
            RenderLayers::layer(LAYER).with(PARTS),
            crate::lifecycle_view::OverlayCamera,
        ))
        .id()
}

fn spawn(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    screens: Query<Entity, With<crate::ui_view::UiCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(icons) = pending.0.take() else {
        return;
    };
    let (camera, own) = match screens.iter().next() {
        Some(screens) => (screens, false),
        None => (own_camera(&mut commands), true),
    };
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let mut part = |rect: [f32; 4], depth: f32, tint: Vec4| {
        let material = materials.add(unlit(tint, None));
        let entity = commands
            .spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(material.clone()),
                gui_quad(rect, depth),
                RenderLayers::layer(PARTS),
                // Hidden until it has something to show (the bars' shades
                // are shown at once).
                if depth == 0.0 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ))
            .id();
        Part {
            entity,
            material: material.id(),
        }
    };
    let shade = Vec4::new(0.0, 0.0, 0.0, 0.55);
    for back in [HP, BAR] {
        part(back, 0.0, shade);
    }
    let hp = part(HP, 1.0, Vec4::ONE);
    let bar = part(BAR, 1.0, Vec4::ONE);
    let shield = part(SHIELD, 1.0, Vec4::new(0.3, 0.9, 1.0, 0.9));
    // Shown only in a duel mode: their shades hidden until then too.
    let duel = DUEL.map(|rect| (part(rect, 0.5, shade), part(rect, 1.0, Vec4::ONE)));
    let slot = |at: usize| {
        let [left, top, width, height] = SLOT;
        [left + at as f32 * (width + 6.0), top, width, height]
    };
    let slots = std::array::from_fn(|at| part(slot(at), 1.0, Vec4::ONE));
    let pips = std::array::from_fn(|at| {
        let [left, top, width, height] = slot(at);
        std::array::from_fn(|pip| {
            let size = width / PIPS as f32;
            part(
                [
                    left + pip as f32 * size,
                    top + height + 2.0,
                    size - 0.5,
                    3.0,
                ],
                1.0,
                Vec4::new(1.0, 1.0, 1.0, 0.9),
            )
        })
    });
    // Icons are stored bottom row first (TPC): turned over.
    let mut place = gui_quad(POWER, 1.0);
    place.scale.y = -place.scale.y;
    let icons: BTreeMap<u16, Handle<OdysseyMaterial>> = icons
        .iter()
        .map(|(row, icon)| {
            let image = images.add(crate::scene::image(icon));
            (*row, materials.add(unlit(Vec4::ONE, Some(image))))
        })
        .collect();
    let first = icons
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| materials.add(unlit(Vec4::ZERO, None)));
    let power = commands
        .spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(first),
            place,
            RenderLayers::layer(PARTS),
            Visibility::Hidden,
        ))
        .id();
    commands.insert_resource(Hud {
        camera,
        own,
        hp,
        bar,
        shield,
        duel,
        power,
        icons,
        slots,
        pips,
    });
    commands.remove_resource::<Pending>();
}

/// What the label says (`pick_view::Picking::label`).
type Label = (crate::pick_view::LabelOn, Vec<u8>, f32);

/// The bar slots the HUD shows, and the label over one of them: the slot,
/// the words and their opacity (`HudView::bar_label`).
type BarShown = (Vec<ui::vm::BarSlot>, Option<(usize, Vec<u8>, f32)>);

/// The item that each kind's bar slot uses now, by `Shelf`
/// (`pick_view::chosen`). A kind the character has none left of has none.
pub(crate) type Chosen = [Option<crate::pick_view::Kind>; items::Shelf::ALL.len()];

/// How a bar slot stands this frame, apart from its icon and its key.
#[derive(Debug, Clone, Copy, Default)]
struct SlotNow {
    /// The character can use what the slot holds now and has the Force
    /// points to start it (`bar_view::usable`, `bar_view::payable`).
    ready: bool,
    /// What the slot holds is on (`bar_view::lit`).
    on: bool,
}

/// An item template of each kind, by `Shelf`, whose icon a kind's slot
/// shows while the character has none of that kind left. Each list is tried
/// in order, because the two games name their energy shields differently.
pub(crate) const SHELF_ICONS: [&[&str]; 4] = [
    &["g_i_medeqpmnt01"],
    &["g_i_adrnaline001"],
    &["g_i_frarmbnds01", "a_shield_01"],
    &["g_w_fraggren01"],
];
/// The item template whose icon the mine slot shows, a mine kit that both
/// games have.
pub(crate) const MINE_ICON: &str = "g_i_trapkit001";
/// The HUD's own stealth toggle picture (`TB_STEALTH`), which the stealth
/// mode slot shows.
const STEALTH_ICON: &str = "i_mistealth";

/// The icon of the first of `templates` that names an item.
pub(crate) fn item_icon<'a>(
    assets: &mut ui::assets::GuiAssets,
    templates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    templates
        .into_iter()
        .find_map(|template| Some(assets.item_ref(template)?.icon.clone()))
}

/// The icon of what a bar slot holds. A power shows its `spells.2da` icon,
/// and pull shows push's. A kind of item shows the icon of the item that
/// its slot uses now, or a stock item of the kind when the character has
/// none left. A feat attack shows the `feat.2da` icon of the highest tier
/// the character knows.
pub(crate) fn bar_icon(
    assets: &mut ui::assets::GuiAssets,
    content: sim::combat::bar::Content,
    sheet: &sim::combat::Sheet,
    chosen: &Chosen,
) -> Option<String> {
    use sim::combat::bar::Content;
    match content {
        Content::Power(row) => {
            let row = if row == force::PULL { PUSH } else { row };
            assets.cell("spells", usize::from(row), "iconresref")
        }
        Content::Item(shelf) => {
            let template = chosen[shelf as usize]
                .as_ref()
                .and_then(|kind| kind.template.as_deref());
            let stock = SHELF_ICONS[shelf as usize].iter().copied();
            item_icon(assets, template.into_iter().chain(stock))
        }
        Content::Mine => item_icon(assets, [MINE_ICON]),
        Content::Feat(line) => {
            let row = line.known(sheet).or(line.rows().first().copied())?;
            assets.cell("feat", usize::from(row), "icon")
        }
        Content::Stealth => Some(STEALTH_ICON.into()),
    }
}

/// The name the power wheel and the bar setup screen show for `content`. A
/// power shows its `spells.2da` name. A kind of item shows the name of the
/// item its slot uses now (`chosen`). Mines show the mine kit's name. A
/// feat attack shows the name of the highest tier the character knows, and
/// stealth mode shows the Stealth skill's name. A content with no name in
/// the game's tables shows none.
pub(crate) fn bar_name(
    ui: &mut crate::ui_view::Ui,
    content: sim::combat::bar::Content,
    sheet: &sim::combat::Sheet,
    chosen: &Chosen,
) -> Vec<u8> {
    use crate::pick_view::{item_name, table_name};
    use sim::combat::bar::Content;
    let name = match content {
        Content::Power(row) => table_name(ui, "spells", usize::from(row)),
        Content::Item(shelf) => chosen[shelf as usize]
            .as_ref()
            .and_then(|kind| item_name(ui, kind)),
        Content::Mine => ui
            .assets
            .as_mut()
            .and_then(|assets| Some(assets.item_ref(MINE_ICON)?.name.clone())),
        Content::Feat(line) => line
            .known(sheet)
            .or(line.rows().first().copied())
            .and_then(|row| table_name(ui, "feat", usize::from(row))),
        Content::Stealth => table_name(ui, "skills", sim::combat::sheet::skills::STEALTH),
    };
    name.unwrap_or(Vec::new())
}

/// What the power wheel and the bar setup screen show of `content` for the
/// character `means` describes: its icon (`bar_icon`), its name
/// (`bar_name`), how many are left of an item kind or of the party's mine
/// kits (`kits`), whether the character cannot use it or pay for it now,
/// and whether it is on.
pub(crate) fn bar_entry(
    ui: &mut crate::ui_view::Ui,
    content: sim::combat::bar::Content,
    means: &crate::bar_view::Means,
    chosen: &Chosen,
    kits: u32,
) -> ui::vm::WheelEntry {
    use sim::combat::bar::Content;
    let sheet = &means.sheet;
    let icon = ui
        .assets
        .as_mut()
        .and_then(|assets| bar_icon(assets, content, sheet, chosen));
    let count = match content {
        Content::Item(shelf) => chosen[shelf as usize].as_ref().map(|kind| kind.left),
        Content::Mine => Some(kits),
        _ => None,
    };
    ui::vm::WheelEntry {
        icon: icon.unwrap_or(String::new()),
        name: bar_name(ui, content, sheet, chosen),
        count,
        dim: !crate::bar_view::usable(content, means) || !crate::bar_view::payable(content, means),
        lit: crate::bar_view::lit(content, sheet, means.fighter),
        ..ui::vm::WheelEntry::default()
    }
}

/// What the game says of `content`, for the bar setup screen: a power's
/// description from `spells.2da`, a feat attack's from `feat.2da` (the
/// line's rank the character knows), or the description of the item that
/// the kind's slot uses now. Stealth mode and mines have none, and show
/// only their names.
pub(crate) fn bar_description(
    ui: &mut crate::ui_view::Ui,
    content: sim::combat::bar::Content,
    sheet: &sim::combat::Sheet,
    chosen: &Chosen,
) -> Vec<u8> {
    use sim::combat::bar::Content;
    let Some(assets) = ui.assets.as_mut() else {
        return Vec::new();
    };
    let told = match content {
        Content::Power(row) => assets.cell_text("spells", usize::from(row), "spelldesc"),
        Content::Feat(line) => line
            .known(sheet)
            .or(line.rows().first().copied())
            .and_then(|row| assets.cell_text("feat", usize::from(row), "description")),
        Content::Item(shelf) => chosen[shelf as usize]
            .as_ref()
            .and_then(|kind| kind.template.as_ref())
            .and_then(|template| Some(assets.item_ref(template)?.description.clone())),
        Content::Mine | Content::Stealth => None,
    };
    told.unwrap_or(Vec::new())
}

/// The item that each kind's bar slot uses now for the party member
/// `body` (`pick_view::chosen`). `picking` holds the picks this player
/// asked for that the host has not taken yet. Only the player's own body
/// has such picks.
pub(crate) fn chosen_items(
    snapshot: &sim::Snapshot,
    (body, sheet, fighter): (sim::SimId, &sim::combat::Sheet, &sim::combat::Fighter),
    library: Option<&std::sync::Arc<sim::script::Library>>,
    picking: Option<&crate::pick_view::Picking>,
) -> Chosen {
    let kinds = crate::pick_view::kinds(snapshot, body, sheet, fighter, library);
    items::Shelf::ALL.map(|shelf| {
        let pending = picking.and_then(|picking| picking.pending(shelf));
        let now_ms = snapshot.time_ms;
        let on = crate::pick_view::chosen(&kinds, sheet, fighter, shelf, pending, now_ms);
        on.and_then(|at| kinds[shelf as usize].get(at).cloned())
    })
}

/// How each bar slot of the character `means` describes stands now.
fn slots_now(means: &crate::bar_view::Means) -> [SlotNow; sim::combat::bar::SLOTS] {
    means.sheet.bar.slots().map(|content| {
        content.map_or(SlotNow::default(), |content| SlotNow {
            ready: crate::bar_view::usable(content, means)
                && crate::bar_view::payable(content, means),
            on: crate::bar_view::lit(content, &means.sheet, means.fighter),
        })
    })
}

/// The bar of the character `means` describes, as the bar setup screen
/// shows it: one slot for each of `keys`, with no slot held and no label.
pub(crate) fn setup_slots(
    ui: &mut crate::ui_view::Ui,
    means: &crate::bar_view::Means,
    chosen: &Chosen,
    keys: Vec<ui::vm::KeyLabel>,
) -> Vec<ui::vm::BarSlot> {
    let now = slots_now(means);
    bar_view(ui, &means.sheet, chosen, &now, keys, (None, None), None).0
}

/// The action bar as the HUD shows it: its first `keys.len()` slots, each
/// with its key. `now` says how each slot stands. `held` is the slot whose
/// key is held (`bar_view::BarUse::held`), and `held_shelf` the item kind
/// whose key the item picker holds back (`pick_view::Picking::held_shelf`).
/// Both show their slot lit. The label goes over the slot that holds the
/// kind of item it names, or over the feat attack slot that is on (else
/// the first feat attack slot) when it names a feat attack.
fn bar_view(
    ui: &mut crate::ui_view::Ui,
    sheet: &sim::combat::Sheet,
    chosen: &Chosen,
    now: &[SlotNow; sim::combat::bar::SLOTS],
    keys: Vec<ui::vm::KeyLabel>,
    (held, held_shelf): (Option<usize>, Option<items::Shelf>),
    label: Option<Label>,
) -> BarShown {
    use crate::pick_view::LabelOn;
    use sim::combat::bar::Content;
    let contents = sheet.bar.slots();
    let slots = keys
        .into_iter()
        .enumerate()
        .map(|(at, key)| {
            let content = contents[at];
            let icon = content
                .zip(ui.assets.as_mut())
                .and_then(|(content, assets)| bar_icon(assets, content, sheet, chosen));
            let count = match content {
                Some(Content::Item(shelf)) => {
                    Some(chosen[shelf as usize].as_ref().map_or(0, |kind| kind.left))
                }
                _ => None,
            };
            let pressed = held == Some(at)
                || held_shelf.is_some_and(|shelf| content == Some(Content::Item(shelf)));
            ui::vm::BarSlot {
                icon,
                count,
                key,
                dim: content.is_some() && !now[at].ready,
                lit: pressed || now[at].on,
            }
        })
        .collect::<Vec<_>>();
    let shown = &contents[..slots.len()];
    let label = label.and_then(|(on, words, alpha)| {
        let at = match on {
            LabelOn::Item(shelf) => shown
                .iter()
                .position(|content| *content == Some(Content::Item(shelf))),
            LabelOn::Feat => {
                let feat = |at: &usize| matches!(shown[*at], Some(Content::Feat(_)));
                let mut feats = (0..shown.len()).filter(feat);
                feats
                    .clone()
                    .find(|at| now[*at].on)
                    .or_else(|| feats.next())
            }
        }?;
        Some((at, words, alpha))
    });
    (slots, label)
}

/// What KOTOR's HUD shows: the bars (Force only for a Force user), the
/// portrait of the character played (the player's persona's, or a
/// companion's template's while it leads or is driven) and the action bar
/// (`bar_view`).
fn kotor_view(
    ui: &mut crate::ui_view::Ui,
    sheet: &sim::combat::Sheet,
    fighter: &sim::combat::Fighter,
    (bar, bar_label): BarShown,
    (persona, made, template): (
        Option<&sim::persona::Persona>,
        Option<&crate::character::Made>,
        Option<&str>,
    ),
) -> ui::vm::HudView {
    let mut portrait = made.and_then(|made| made.0.as_ref().map(|made| made.portrait.clone()));
    if let Some(assets) = ui.assets.as_mut() {
        // The portrait darkens with the character's alignment
        // (`alignment_look`).
        let step = crate::alignment_look::in_game(assets.game, sheet.good_evil).step;
        if let Some(template) = template {
            portrait = assets.portrait_of(template, step);
        } else if let Some(persona) = persona {
            portrait = assets
                .portrait(usize::from(persona.portrait), step)
                .or(portrait);
        }
    }
    ui::vm::HudView {
        hp: (fighter.whole_hp(), i32::from(sheet.max_hp)),
        fp: if sheet.max_fp > 0 {
            (fighter.fp / 100, i32::from(sheet.pool()))
        } else {
            (0, 0)
        },
        portrait,
        bar,
        bar_label,
        stealth: fighter.covert.stealth,
        poisoned: fighter.pack.poison.is_some(),
        ..ui::vm::HudView::default()
    }
}

/// The item templates whose icons stand for a droid's kit: the ion
/// grenade (the shock bolt), the sonic grenade (the Sonic Blast) and a
/// medpac (the stim dart).
const REMOTE_ICONS: [&str; 3] = ["g_w_iongren01", "g_w_sonicgren01", "g_i_medeqpmnt01"];
/// Each button's name, over its slot as it fires; and what the stim dart
/// says pressed with no one to treat (no one down or hurt in reach).
const REMOTE_NAMES: [&[u8]; 3] = [b"Shock Bolt", b"Sonic Blast", b"Stim Dart"];
const NO_ONE_HURT: &[u8] = b"No one hurt";
/// How long after a dart is asked for the HUD waits to see it fire before
/// it says no one was hurt (seconds): a round trip and more.
const DART_WAIT: f32 = 0.5;

/// What the droid's HUD last saw of its kit: the buttons' ready times, the
/// slot it last named (its words, since when, seconds), and a stim dart
/// asked for (since when, its ready time then).
#[derive(Default)]
struct Kit {
    ready: Option<[u64; 3]>,
    named: Option<(usize, &'static [u8], f32)>,
    asked: Option<(f32, u64)>,
}

impl Kit {
    /// Notes what `remote` did since the last frame at `now` (seconds,
    /// the sim at `sim_ms`), the stim dart's key (action bar slot 1)
    /// pressed or not; the label over a slot, faded.
    fn note(
        &mut self,
        remote: &sim::remote::Remote,
        (now, sim_ms): (f32, u64),
        asked: bool,
    ) -> Option<(usize, Vec<u8>, f32)> {
        use sim::remote::KIT_DART;
        let before = self.ready.replace(remote.ready_ms);
        for kit in 0..3 {
            if before.is_some_and(|before| remote.ready_ms[kit] > before[kit]) {
                self.named = Some((kit, REMOTE_NAMES[kit], now));
            }
        }
        if asked && remote.ready_ms[KIT_DART] <= sim_ms {
            self.asked = Some((now, remote.ready_ms[KIT_DART]));
        }
        if let Some((since, ready)) = self.asked {
            if remote.ready_ms[KIT_DART] != ready {
                self.asked = None;
            } else if now - since > DART_WAIT {
                self.asked = None;
                self.named = Some((KIT_DART, NO_ONE_HURT, now));
            }
        }
        let (kit, words, since) = self.named?;
        let alpha = ((KIT_SHOWN - (now - since)) / KIT_FADE).clamp(0.0, 1.0);
        (alpha > 0.0).then(|| (kit, words.to_vec(), alpha))
    }
}

/// KOTOR's HUD for a seat flying a remote droid: its three buttons, each
/// with the seconds left before it is ready (none when it is), and the
/// name of the one last fired over it (`Kit`).
fn remote_view(
    ui: &mut crate::ui_view::Ui,
    remote: &sim::remote::Remote,
    now_ms: u64,
    prompts: &Prompts,
    label: Option<(usize, Vec<u8>, f32)>,
) -> ui::vm::HudView {
    let keys = [
        prompts.label(Action::Attack),
        prompts.label(Action::AltAttack),
        prompts
            .bound_label(crate::settings::BAR[0])
            .unwrap_or(ui::vm::KeyLabel::default()),
    ];
    let bar = REMOTE_ICONS
        .iter()
        .zip(remote.ready_ms)
        .zip(keys)
        .map(|((&template, ready), key)| {
            let left = ready.saturating_sub(now_ms);
            ui::vm::BarSlot {
                icon: ui
                    .assets
                    .as_mut()
                    .and_then(|assets| item_icon(assets, [template])),
                count: (left > 0).then(|| left.div_ceil(1000) as u32),
                key,
                ..ui::vm::BarSlot::default()
            }
        })
        .collect();
    ui::vm::HudView {
        bar,
        bar_label: label,
        ..ui::vm::HudView::default()
    }
}

/// What `seat`'s player is in, for its portrait's icon (R10): only in a
/// session of more than one seat (one player's HUD stays as it was).
pub(crate) fn busy(snapshot: &sim::Snapshot, seat: sim::Seat) -> Option<ui::vm::Busy> {
    use sim::Busy;
    let busy = snapshot.busy(seat).filter(|_| snapshot.seats() > 1)?;
    Some(match busy {
        Busy::Talking => ui::vm::Busy::Talking,
        Busy::Shopping => ui::vm::Busy::Shopping,
        Busy::Pazaak => ui::vm::Busy::Pazaak,
        Busy::Asking => ui::vm::Busy::Asking,
    })
}

/// Push's `spells.2da` row: pull shows its icon.
const PUSH: u16 = 23;

/// An item kind's colour.
fn colour(kind: Use) -> Vec4 {
    match kind {
        Use::Grenade => Vec4::new(1.0, 0.55, 0.1, 0.95),
        Use::Heal { .. } | Use::Antidote | Use::Squad { .. } => Vec4::new(0.3, 0.9, 0.35, 0.95),
        Use::Shield { .. } => Vec4::new(0.3, 0.9, 1.0, 0.95),
        Use::Stim(_) => Vec4::new(1.0, 0.9, 0.2, 0.95),
    }
}

/// A bar's quad filled `done` (0 to 1) from the left.
fn filled(rect: [f32; 4], done: f32) -> Transform {
    let [left, top, width, height] = rect;
    gui_quad(
        [left, top, (width * done.clamp(0.0, 1.0)).max(0.01), height],
        1.0,
    )
}

/// Hands the HUD to the screens' camera once there is one; keeps a camera
/// of its own drawing into the scene camera's target (an image when
/// headless): copied from each scene camera once.
fn follow(
    mut commands: Commands,
    hud: Option<ResMut<Hud>>,
    screens: Query<Entity, With<crate::ui_view::UiCamera>>,
    scene: Query<(Entity, Option<&bevy::camera::RenderTarget>), crate::lifecycle_view::SceneCamera>,
    mut copied: Local<Option<Entity>>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    if hud.own
        && let Some(screens) = screens.iter().next()
    {
        commands.entity(hud.camera).despawn();
        hud.camera = screens;
        hud.own = false;
    }
    let Some((camera, target)) = scene.iter().next().filter(|_| hud.own) else {
        return;
    };
    if *copied != Some(camera) {
        *copied = Some(camera);
        if let Some(target) = target {
            commands.entity(hud.camera).insert(target.clone());
        }
    }
}

/// KOTOR's HUD's side: its view-model, the portrait made, whether it is
/// drawn, the item picker, the clock, the bar slot in use, the layers
/// cameras draw (this HUD's parts stand down), the scene (its area's map).
type KotorSide<'w, 's> = (
    Option<ResMut<'w, crate::ui_view::Ui>>,
    Option<Res<'w, crate::character::Made>>,
    Option<Res<'w, crate::ui_view::KotorHud>>,
    (
        Option<Res<'w, crate::pick_view::Picking>>,
        Res<'w, Time<Real>>,
        Option<Res<'w, crate::bar_view::BarUse>>,
    ),
    Query<'w, 's, &'static mut RenderLayers>,
    Option<Res<'w, crate::scene::SceneContent>>,
);

/// How long a droid kit's name shows over its slot, and the fade at its
/// end (seconds).
const KIT_SHOWN: f32 = 2.0;
const KIT_FADE: f32 = 0.5;

#[allow(clippy::too_many_arguments)]
fn show(
    mut droid_kit: Local<Kit>,
    hud: Option<Res<Hud>>,
    presented: Res<PresentedSnapshot>,
    own: Option<Res<OwnSeat>>,
    selected: Option<Res<SelectedPower>>,
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    prompts: Prompts,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
    mut icon: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut ui, made, kotor, (picking, real, bar_use), mut cameras, scene): KotorSide,
) {
    let (Some(hud), Some(own)) = (hud, own) else {
        return;
    };
    let snapshot = presented.0.current();
    let Some((entity, sheet)) = swap
        .as_ref()
        .and_then(|swap| crate::cast::body_sheet(snapshot, own.0, &swap.level))
    else {
        // No body: a seat flying a droid has its kit's three buttons.
        if let Some(ui) = ui.as_deref_mut() {
            let now = real.elapsed_secs();
            let asked = prompts.pressed(crate::settings::BAR[0]);
            ui.vm.hud = snapshot
                .remote(own.0)
                .filter(|remote| !remote.hidden)
                .map(|remote| {
                    let label = droid_kit.note(remote, (now, snapshot.time_ms), asked);
                    remote_view(ui, remote, snapshot.time_ms, &prompts, label)
                });
            let kit = ui.vm.hud.as_ref().map(|hud| {
                serde_json::json!({
                    "icons": hud.bar.iter().map(|slot| slot.icon.clone().unwrap_or(String::new())).collect::<Vec<_>>(),
                    "label": hud.bar_label.as_ref()
                        .map(|(at, words, _)| (at, String::from_utf8_lossy(words).into_owned())),
                })
            });
            facts
                .0
                .insert("hud".into(), serde_json::json!({ "remote": kit }));
        }
        return;
    };
    let Some(fighter) = entity.fighter else {
        return;
    };
    // The power used last, among those its sheet knows.
    let library = swap.as_ref().and_then(|swap| swap.level.scripts().cloned());
    let choices = force::choices(&sheet.powers, library.as_deref());
    let power = selected
        .map(|selected| selected.0)
        .filter(|row| choices.contains(row))
        .or_else(|| choices.first().copied());
    let material = power.and_then(|row| hud.icons.get(&row));
    if let Ok((_, mut visibility)) = parts.get_mut(hud.power) {
        *visibility = if material.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let (Some(material), Ok(mut handle)) = (material, icon.get_mut(hud.power))
        && handle.0 != *material
    {
        handle.0 = material.clone();
    }
    let mut set = |part: Part, place: Option<Transform>, tint: Option<Vec4>, shown: bool| {
        if let Ok((mut transform, mut visibility)) = parts.get_mut(part.entity) {
            if let Some(place) = place {
                *transform = place;
            }
            *visibility = if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if let Some(tint) = tint
            && materials
                .get(part.material)
                .is_some_and(|material| material.params.tint != tint)
            && let Some(mut material) = materials.get_mut(part.material)
        {
            material.params.tint = tint;
        }
    };
    let health = fighter.hp as f32 / (f32::from(sheet.max_hp) * 100.0);
    set(
        hud.hp,
        Some(filled(HP, health)),
        Some(Vec4::new(1.0 - health, 0.25 + 0.65 * health, 0.2, 0.95)),
        true,
    );
    let pool = f32::from(sheet.pool()) * 100.0;
    let bar = if sheet.max_fp > 0 {
        Vec4::new(0.25, 0.45, 1.0, 0.95)
    } else {
        Vec4::new(0.95, 0.75, 0.2, 0.95)
    };
    set(
        hud.bar,
        Some(filled(BAR, fighter.fp as f32 / pool)),
        Some(bar),
        true,
    );
    let shield = fighter
        .pack
        .shield
        .and_then(|shield| match items::use_of(shield.row) {
            Some(Use::Shield { points, .. }) => {
                Some(f32::from(shield.left) / f32::from(points.max(1)))
            }
            _ => None,
        });
    set(
        hud.shield,
        shield.map(|left| filled(SHIELD, left)),
        None,
        shield.is_some(),
    );
    // The duel bars, while a duel mode is played with a saber in hand.
    let rules = snapshot.saber_rules;
    let duel = (rules.duel() && sim::combat::forms::saber(&sheet))
        .then(|| crate::duel_view::bars(&fighter, real.elapsed_secs_f64() * 1000.0));
    let rows = [
        duel.as_ref().and_then(|duel| duel.points),
        duel.as_ref().map(|duel| duel.stamina),
    ];
    for (((shade, part), rect), row) in hud.duel.iter().zip(DUEL).zip(rows) {
        set(*shade, None, None, row.is_some());
        set(
            *part,
            row.map(|row| filled(rect, row.share)),
            row.map(|row| Vec4::from_array(row.color)),
            row.is_some(),
        );
    }
    // The items: a slot by kind, a pip a use left.
    let mut carried = Vec::new();
    for (at, item) in sheet.items.iter().enumerate() {
        let kind = item.and_then(|item| items::use_of(item.row).map(|kind| (item, kind)));
        let left = kind.map_or(0, |(item, _)| {
            item.count.saturating_sub(fighter.pack.used[at])
        });
        set(
            hud.slots[at],
            None,
            kind.map(|(_, kind)| colour(kind)),
            kind.is_some(),
        );
        for (pip, part) in hud.pips[at].iter().enumerate() {
            set(*part, None, None, kind.is_some() && pip < usize::from(left));
        }
        if let Some((item, _)) = kind {
            carried.push(serde_json::json!({"row": item.row, "left": left}));
        }
    }
    let library = swap.as_ref().and_then(|swap| swap.level.scripts());
    let chosen = chosen_items(
        snapshot,
        (entity.id, &sheet, &fighter),
        library,
        picking.as_deref(),
    );
    // How each bar slot stands, from what the character can use now.
    let now = swap
        .as_ref()
        .and_then(|swap| {
            crate::bar_view::with_bar(snapshot, own.0, &swap.level, |means, _| slots_now(means))
        })
        .unwrap_or([SlotNow::default(); sim::combat::bar::SLOTS]);
    let shown = if prompts.pad() {
        crate::settings::PAD_BAR
    } else {
        crate::settings::BAR.len()
    };
    let keys = crate::settings::BAR[..shown]
        .iter()
        .map(|key| {
            prompts
                .bound_label(*key)
                .unwrap_or(ui::vm::KeyLabel::default())
        })
        .collect();
    if let Ok(mut layers) = cameras.get_mut(hud.camera) {
        let drawn = if kotor.is_some_and(|kotor| kotor.0) {
            RenderLayers::layer(LAYER)
        } else {
            RenderLayers::layer(LAYER).with(PARTS)
        };
        if *layers != drawn {
            *layers = drawn;
        }
    }
    if let Some(ui) = ui.as_deref_mut() {
        let map = scene.as_ref().and_then(|scene| scene.0.map.as_ref());
        // KOTOR 2's feedback option "Mini Map" off hides the minimap.
        let minimap_on = ui::feedback::on(&ui.vm.settings, ui::feedback::MINIMAP);
        let minimap = swap
            .as_ref()
            .filter(|_| minimap_on)
            .and_then(|swap| swap.level.module())
            .zip(ui.assets.as_mut())
            .and_then(|(module, assets)| {
                crate::map_view::minimap(assets, map, module, snapshot, entity)
            });
        if facts.wanted() {
            facts.0.insert(
                "minimap".into(),
                minimap.as_ref().map_or(serde_json::Value::Null, |view| {
                    serde_json::json!({"picture": view.picture, "at": view.at, "spin": view.spin})
                }),
            );
        }
        // A companion played: its template's portrait.
        let template = swap
            .as_ref()
            .filter(|_| entity.kind != sim::Kind::Avatar(own.0))
            .and_then(|swap| {
                let instances = sim::Instances::in_snapshot(&swap.level, snapshot);
                Some(instances.of_entity(entity)?.template.clone())
            });
        let bar = bar_view(
            ui,
            &sheet,
            &chosen,
            &now,
            keys,
            (
                bar_use.as_ref().and_then(|bar_use| bar_use.held),
                picking.as_ref().and_then(|picking| picking.held_shelf()),
            ),
            picking
                .as_ref()
                .and_then(|picking| picking.label(real.elapsed_secs())),
        );
        ui.vm.hud = Some(ui::vm::HudView {
            minimap,
            busy: busy(snapshot, own.0),
            solo: snapshot.solo,
            duel: duel.clone(),
            ..kotor_view(
                ui,
                &sheet,
                &fighter,
                bar,
                (
                    sim::persona::of(&snapshot.personas, own.0),
                    made.as_deref(),
                    template.as_deref(),
                ),
            )
        });
    }
    if facts.wanted() {
        facts.0.insert(
            "hud".into(),
            serde_json::json!({
                "hp": health, "bar": fighter.fp as f32 / pool,
                "power": power, "powers": choices, "shield": shield, "items": carried,
                "icons": hud.icons.len(),
                "solo": snapshot.solo,
                "duel": duel.as_ref().map(|duel| serde_json::json!({
                    "points": duel.points.map(|bar| bar.share),
                    "stamina": duel.stamina.share,
                })),
                "busy": (0..snapshot.seats())
                    .map(|seat| busy(snapshot, sim::Seat(seat)).map(|busy| format!("{busy:?}")))
                    .collect::<Vec<_>>(),
            }),
        );
    }
}
