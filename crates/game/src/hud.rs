//! The fight's HUD in `kjk play` (render side only: it reads the
//! presented snapshots and `sim_host::SelectedPower`), laid out in KOTOR's
//! 640 × 480 GUI space at the bottom left:
//!
//! - the player's hit points (green, turning red as they fall) and its
//!   second bar (blue for FP, gold for stamina), the energy shield's points
//!   left (cyan) while one is up;
//! - the selected power's KOTOR icon (`spells.2da` `iconresref`; pull shows
//!   push's), for a character that knows powers;
//! - the four kinds of items it carries, as slots coloured by kind (grenade
//!   orange, healing green, shield cyan, stim yellow), a pip a use left (at
//!   most eight).
//!
//! It is drawn by a camera of its own over the scene, as the loading screen
//! is (`lifecycle_view`), so it needs no UI plugin and shows in headless
//! captures. Where KOTOR's own HUD is drawn (`ui_view::KotorHud`) this one
//! stands down; it fills that one's view-model (`HudView`): the bars, the
//! portrait made in creation, the selected power and the items as action
//! slots, and the minimap (the area's picture, where the player stands on
//! it and which way it faces).

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
use crate::settings::{Action, KeyMap};
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
/// The power icon, and the first item slot (the rest follow to its right).
const POWER: [f32; 4] = [174.0, 430.0, 32.0, 32.0];
const SLOT: [f32; 4] = [214.0, 432.0, 20.0, 20.0];
const PIPS: usize = 8;

/// Each power row's icon (by `Command::force`: pull's is push's), and the
/// conversation's talk table and install (`dialogue_view`).
pub struct HudSetup {
    icons: BTreeMap<u8, TextureIr>,
    dialogue: crate::dialogue_view::Setup,
}

/// The icons of every `spells.2da` row the sim maps to a power, read from
/// `game`'s install (a row whose icon does not read has none): through
/// `installer` (the stage's, open already), else one opened here.
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
        for row in 0..table.cells.len().min(usize::from(force::PULL)) {
            let row = row as u8;
            if force::of_row(u16::from(row)).is_none() {
                continue;
            }
            let icon = table
                .resref(usize::from(row), "iconresref")
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
struct Pending(Option<BTreeMap<u8, TextureIr>>);

/// The HUD's parts.
#[derive(Resource)]
struct Hud {
    /// Its camera, drawing into what the scene's camera draws into.
    camera: Entity,
    hp: Part,
    bar: Part,
    shield: Part,
    power: Entity,
    /// Each power row's material (its icon).
    icons: BTreeMap<u8, Handle<OdysseyMaterial>>,
    slots: [Part; KINDS],
    pips: [[Part; PIPS]; KINDS],
}

/// A quad, and its own material (its tint changes).
#[derive(Clone, Copy)]
struct Part {
    entity: Entity,
    material: AssetId<OdysseyMaterial>,
}

fn spawn(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(icons) = pending.0.take() else {
        return;
    };
    let camera = commands.spawn((
        Camera3d::default(),
        crate::scene::over_world(11),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 480.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        crate::scene::encoded_view(),
        Transform::from_xyz(0.0, 0.0, 100.0),
        RenderLayers::layer(LAYER).with(PARTS),
        crate::lifecycle_view::OverlayCamera,
    ));
    let camera = camera.id();
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
    let icons: BTreeMap<u8, Handle<OdysseyMaterial>> = icons
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
        hp,
        bar,
        shield,
        power,
        icons,
        slots,
        pips,
    });
    commands.remove_resource::<Pending>();
}

/// An item slot of the HUD: its `spells.2da` row, the uses left, its key.
type Slot = (u16, u32, Vec<u8>);

/// The key that uses an item of `kind`.
fn key_of(kind: items::Use) -> Action {
    match kind {
        Use::Grenade => Action::Grenade,
        Use::Heal { .. } | Use::Antidote | Use::Squad { .. } => Action::Medpac,
        Use::Shield { .. } => Action::Shield,
        Use::Stim(_) => Action::Stim,
    }
}

/// What KOTOR's HUD shows: the bars (Force only for a Force user), the
/// portrait of the character played (the player's persona's, or a
/// companion's template's while it leads or is driven), the power then
/// each item kind carried as action slots
/// (the power's `spells.2da` icon, pull showing push's; an item kind's the
/// icon of the party's first item that casts it), then TSL's stance column
/// for one who knows a Form: the Form in use, else the first it knows
/// (`form_view` picks the next).
fn kotor_view(
    ui: &mut crate::ui_view::Ui,
    sheet: &sim::combat::Sheet,
    fighter: &sim::combat::Fighter,
    (power, slots): (Option<(u8, Vec<u8>)>, &[Slot]),
    goods: &sim::inventory::Goods,
    (persona, made, template): (
        Option<&sim::persona::Persona>,
        Option<&crate::character::Made>,
        Option<&str>,
    ),
) -> ui::vm::HudView {
    let mut actions = Vec::new();
    let mut keys = Vec::new();
    let mut portrait = made.and_then(|made| made.0.as_ref().map(|made| made.portrait.clone()));
    if let Some(assets) = ui.assets.as_mut() {
        if let Some(template) = template {
            portrait = assets.portrait_of(template);
        } else if let Some(persona) = persona {
            portrait = assets
                .cell("portraits", usize::from(persona.portrait), "baseresref")
                .or(portrait);
        }
        let power = power.map(|(row, key)| (if row == force::PULL { PUSH } else { row }, key));
        if let Some((name, key)) = power.and_then(|(row, key)| {
            Some((assets.cell("spells", usize::from(row), "iconresref")?, key))
        }) {
            actions.push((name, None));
            keys.push(key);
        }
        for (row, left, key) in slots {
            keys.push(key.clone());
            let icon = goods.carried(sim::inventory::PARTY).find_map(|(_, item)| {
                let look = assets.item_ref(&item.template)?;
                (look.casts == Some(*row)).then(|| look.icon.clone())
            });
            actions.push((icon.unwrap_or(String::new()), Some(*left)));
        }
        let stance = Some(sheet.form)
            .filter(|form| *form != 0)
            .or_else(|| crate::form_view::known(sheet).next());
        if let Some(icon) =
            stance.and_then(|form| assets.cell("spells", usize::from(form), "iconresref"))
        {
            actions.push((icon, None));
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
        actions,
        action_keys: keys,
        stealth: fighter.covert.stealth,
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
const PUSH: u8 = 23;

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

/// Keeps the HUD's camera drawing into the scene camera's target (an
/// image when headless): copied from each scene camera once.
fn follow(
    mut commands: Commands,
    hud: Option<Res<Hud>>,
    scene: Query<(Entity, Option<&bevy::camera::RenderTarget>), crate::lifecycle_view::SceneCamera>,
    mut copied: Local<Option<Entity>>,
) {
    let (Some(hud), Some((camera, target))) = (hud, scene.iter().next()) else {
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
/// drawn, the layers cameras draw (this HUD's parts stand down), the scene
/// (its area's map).
type KotorSide<'w, 's> = (
    Option<ResMut<'w, crate::ui_view::Ui>>,
    Option<Res<'w, crate::character::Made>>,
    Option<Res<'w, crate::ui_view::KotorHud>>,
    Query<'w, 's, &'static mut RenderLayers>,
    Option<Res<'w, crate::scene::SceneContent>>,
);

/// When the player last picked another power (`power_pick`): the power
/// picked, and when (seconds of the app's clock); none before the first.
#[derive(Default)]
struct Picked {
    power: Option<u8>,
    at: Option<f32>,
}

/// How long the carousel shows after a pick, and the fade at its end
/// (seconds).
const PICK_SHOWN: f32 = 2.0;
const PICK_FADE: f32 = 0.5;

/// The carousel (`ui::screens::power_pick`) of `choices` with `power`
/// chosen, faded to `alpha`: each power's `spells.2da` name (pull's JA's
/// own), the chosen one's cost and place under them.
fn power_pick(
    assets: &mut ui::assets::GuiAssets,
    choices: &[u8],
    power: u8,
    alpha: f32,
) -> ui::vm::PowerPickView {
    let names = choices
        .iter()
        .map(|&row| match row {
            force::PULL => b"Force Pull".to_vec(),
            row => assets
                .cell_text("spells", usize::from(row), "name")
                .unwrap_or_else(|| format!("Power {row}").into_bytes()),
        })
        .collect();
    let chosen = choices.iter().position(|row| *row == power).unwrap_or(0);
    let paid = if power == force::PULL { PUSH } else { power };
    let cost = assets
        .cell("spells", usize::from(paid), "forcepoints")
        .unwrap_or_else(|| "?".to_string());
    ui::vm::PowerPickView {
        names,
        chosen,
        under: format!("{cost} FP    {} / {}", chosen + 1, choices.len()).into_bytes(),
        alpha,
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    (time, mut picked): (Res<Time>, Local<Picked>),
    hud: Option<Res<Hud>>,
    presented: Res<PresentedSnapshot>,
    own: Option<Res<OwnSeat>>,
    selected: Option<Res<SelectedPower>>,
    swap: Option<Res<crate::sim_host::SceneSwap>>,
    keymap: Option<Res<KeyMap>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
    mut icon: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut ui, made, kotor, mut cameras, scene): KotorSide,
) {
    let (Some(hud), Some(own)) = (hud, own) else {
        return;
    };
    let snapshot = presented.0.current();
    let Some((entity, sheet)) = swap
        .as_ref()
        .and_then(|swap| crate::cast::body_sheet(snapshot, own.0, &swap.level))
    else {
        return;
    };
    let Some(fighter) = entity.fighter else {
        return;
    };
    let keymap = keymap.as_deref().unwrap_or(KeyMap::defaults());
    // The selected power, among those its sheet knows.
    let choices = force::choices(&sheet.powers);
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
    // The items: a slot by kind, a pip a use left.
    let mut carried = Vec::new();
    let mut slots = Vec::new();
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
        if let Some((item, kind)) = kind {
            carried.push(serde_json::json!({"row": item.row, "left": left}));
            slots.push((item.row, u32::from(left), keymap.shown(key_of(kind))));
        }
    }
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
    // The carousel while the player steps through its powers: shown from
    // a pick on (not the first power, chosen for it), fading.
    let now = time.elapsed_secs();
    if picked.power != power {
        picked.at = picked.power.is_some().then_some(now);
        picked.power = power;
    }
    let alpha = picked.at.map_or(0.0, |at| {
        ((at + PICK_SHOWN - now) / PICK_FADE).clamp(0.0, 1.0)
    });
    if let Some(ui) = ui.as_deref_mut() {
        ui.vm.power_pick = power
            .filter(|_| alpha > 0.0)
            .zip(ui.assets.as_mut())
            .map(|(power, assets)| power_pick(assets, &choices, power, alpha));
        let map = scene.as_ref().and_then(|scene| scene.0.map.as_ref());
        let minimap = swap
            .as_ref()
            .and_then(|swap| swap.level.module())
            .zip(ui.assets.as_mut())
            .and_then(|(module, assets)| {
                crate::map_view::minimap(assets, map, module, snapshot, entity)
            });
        facts.0.insert(
            "minimap".into(),
            minimap.as_ref().map_or(serde_json::Value::Null, |view| {
                serde_json::json!({"picture": view.picture, "at": view.at, "spin": view.spin})
            }),
        );
        // A companion played: its template's portrait.
        let template = swap
            .as_ref()
            .filter(|_| entity.kind != sim::Kind::Avatar(own.0))
            .and_then(|swap| {
                let instances = sim::Instances::in_snapshot(&swap.level, snapshot);
                Some(instances.of_entity(entity)?.template.clone())
            });
        ui.vm.hud = Some(ui::vm::HudView {
            minimap,
            busy: busy(snapshot, own.0),
            ..kotor_view(
                ui,
                &sheet,
                &fighter,
                (power.map(|row| (row, keymap.shown(Action::Power))), &slots),
                &snapshot.goods,
                (
                    sim::persona::of(&snapshot.personas, own.0),
                    made.as_deref(),
                    template.as_deref(),
                ),
            )
        });
    }
    facts.0.insert(
        "hud".into(),
        serde_json::json!({
            "hp": health, "bar": fighter.fp as f32 / pool,
            "power": power, "shield": shield, "items": carried,
            "icons": hud.icons.len(),
            "busy": (0..snapshot.seats())
                .map(|seat| busy(snapshot, sim::Seat(seat)).map(|busy| format!("{busy:?}")))
                .collect::<Vec<_>>(),
        }),
    );
}
