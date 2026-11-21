//! The view's markers: an icon over each thing the editor places, in the
//! colour of its kind, drawn over the scene each frame (`docs/LEVEL-EDIT.md`,
//! "The editor's window"). A click on an icon selects its thing before the
//! scene's models are looked at (`under`).
//!
//! The icons are line-md's (`icons/`, MIT): each the last frame of its
//! animation, drawn in white and tinted here.
//!
//! | Icon | Marks |
//! |---|---|
//! | filled square | a placeable, a Jedi Academy prop |
//! | speaker | a sound |
//! | arrow down | a waypoint |
//! | map marker | a place a character is put: a waypoint whose tag names a party member, the player or a post (`post_`), and a new map's entry |
//! | person | a creature with a conversation |
//! | person with an alert | a creature whose conversation updates the journal |
//! | alert circle | a trigger, an encounter |
//! | sun | a light placed by hand, a fitted light, a lamp |
//! | dot | a creature with no conversation, a door, a store |

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy_egui::egui;
use level_edit::{Kind, Target};

use kjk_game::scene::game_to_bevy;

use crate::state::{self, EditorState};

/// What an icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Icon {
    Square,
    Sound,
    Waypoint,
    Post,
    Talker,
    Quest,
    Alert,
    Light,
    Dot,
}

/// Each icon's picture, as `icons/` holds it.
const SOURCES: [(Icon, &str); 8] = [
    (Icon::Square, include_str!("../icons/square-filled.svg")),
    (Icon::Sound, include_str!("../icons/volume-high.svg")),
    (
        Icon::Waypoint,
        include_str!("../icons/arrow-down-circle.svg"),
    ),
    (Icon::Post, include_str!("../icons/map-marker-radius.svg")),
    (Icon::Talker, include_str!("../icons/account.svg")),
    (Icon::Quest, include_str!("../icons/account-alert.svg")),
    (Icon::Alert, include_str!("../icons/alert-circle.svg")),
    (Icon::Light, include_str!("../icons/sunny.svg")),
];

/// The side the icons are drawn at once, pixels: large enough for the
/// largest marker on a screen of twice the density.
const DRAWN_SIDE: u32 = 64;

/// The icons as textures, made once for the window's context.
pub struct Icons(BTreeMap<Icon, egui::TextureHandle>);

impl Icons {
    pub fn new(ctx: &egui::Context) -> Icons {
        let mut textures = BTreeMap::new();
        for (icon, source) in SOURCES {
            match picture(source, DRAWN_SIDE) {
                Ok(image) => {
                    let texture = ctx.load_texture(
                        format!("marker {icon:?}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    textures.insert(icon, texture);
                }
                Err(why) => warn!("the marker icon {icon:?} does not draw: {why}"),
            }
        }
        Icons(textures)
    }

    /// Draws `icon` in `rect`, tinted `colour`. A dot, or an icon that did
    /// not draw, is a filled circle.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        icon: Icon,
        rect: egui::Rect,
        colour: egui::Color32,
    ) {
        match self.0.get(&icon) {
            Some(texture) => {
                let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(texture.id(), rect, whole, colour);
            }
            None => {
                painter.circle_filled(rect.center(), rect.width() * 0.3, colour);
            }
        }
    }
}

/// An SVG drawn white on clear, `side` pixels square.
fn picture(source: &str, side: u32) -> Result<egui::ColorImage, String> {
    use resvg::{tiny_skia, usvg};
    let tree =
        usvg::Tree::from_str(source, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side).ok_or("an empty size")?;
    let scale = side as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = (pixmap.pixels().iter())
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [side as usize, side as usize],
        &rgba,
    ))
}

/// The words of a waypoint's tag that make it a place a character is put:
/// the party members of both games, the player, and a post.
const POST_WORDS: [&str; 26] = [
    "carth",
    "bastila",
    "mission",
    "zaalbar",
    "canderous",
    "juhani",
    "jolee",
    "hk",
    "hk47",
    "t3",
    "t3m4",
    "atton",
    "kreia",
    "bao",
    "baodur",
    "mira",
    "hanharr",
    "handmaiden",
    "disciple",
    "visas",
    "mandalore",
    "g0t0",
    "goto",
    "pc",
    "party",
    "post",
];

/// Whether a waypoint tagged `tag` is a place a character is put: a word of
/// its tag (between `_`s, its trailing digits left off) is one of
/// `POST_WORDS`.
pub fn is_post(tag: &str) -> bool {
    tag.to_ascii_lowercase().split('_').any(|word| {
        let word = word.trim_end_matches(|c: char| c.is_ascii_digit());
        POST_WORDS.contains(&word)
    })
}

/// A creature template's icon: by its conversation, which `talks` gives
/// (`None` for none, else whether it updates the journal).
fn creature_icon(talks: Option<bool>) -> Icon {
    match talks {
        None => Icon::Dot,
        Some(false) => Icon::Talker,
        Some(true) => Icon::Quest,
    }
}

/// Whether each creature template of the area has a conversation, and
/// whether that conversation updates the journal (one of its lines has a
/// `Quest`), by template. Read when the scene is drawn.
pub fn creature_talks(state: &EditorState) -> BTreeMap<String, Option<bool>> {
    let mut talks: BTreeMap<String, Option<bool>> = BTreeMap::new();
    let mut quests: BTreeMap<String, bool> = BTreeMap::new();
    for entry in state.editor.entries() {
        if entry.kind != Kind::Creature || talks.contains_key(&entry.template) {
            continue;
        }
        let conversation = state
            .template_gff(&entry.template, fmt_odyssey::ResType::UTC)
            .and_then(|gff| gff.root.resref("Conversation"))
            .map(|name| name.to_ascii_lowercase())
            .filter(|name| !name.is_empty());
        let talk = conversation.map(|name| {
            *quests.entry(name.clone()).or_insert_with(|| {
                let dialog =
                    (state.editor.project.conversations.get(&name).cloned()).or_else(|| {
                        (state.setup.resolver.read(&name, fmt_odyssey::ResType::DLG))
                            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                    });
                dialog.is_some_and(|dialog| {
                    ["EntryList", "ReplyList"].iter().any(|list| {
                        (dialog.root.list(list).iter())
                            .any(|line| line.bytes("Quest").is_some_and(|quest| !quest.is_empty()))
                    })
                })
            })
        });
        talks.insert(entry.template.clone(), talk);
    }
    talks
}

/// One marker as the view shows it now.
pub struct Shown {
    pub target: Target,
    pub icon: Icon,
    /// Its middle on the screen, and its side (points).
    pub at: egui::Pos2,
    pub side: f32,
    /// How far it is from the camera, metres.
    pub distance: f32,
    pub colour: egui::Color32,
}

/// How far away a marker is still drawn, metres.
const FARTHEST: f32 = 80.0;
/// The most markers drawn, the nearest first.
const MOST: usize = 600;

fn colour([r, g, b, _]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// How far (metres) from the level's middle the sun's marker stands, the
/// way the sun stands from it.
const SUN_MARKER: f32 = 40.0;

/// Where the sun's marker stands: `SUN_MARKER` from the level's middle,
/// toward the sun at `at`.
pub fn sun_marker(middle: [f32; 3], at: [f32; 3]) -> [f32; 3] {
    let way = Vec3::from_array(at) - Vec3::from_array(middle);
    (Vec3::from_array(middle) + way.normalize_or_zero() * SUN_MARKER).to_array()
}

/// How far above its place a target's marker stands, metres: over a
/// creature's head, at a light's own place, a little up for the rest.
pub fn rise(target: Target) -> f32 {
    match target {
        Target::Object(Kind::Creature, _) => 2.0,
        Target::Light(_) | Target::Fitted(_) | Target::Lamp(_) => 0.0,
        _ => state::MARKER_RISE,
    }
}

/// Every marker the view shows from `eye`, the farthest first (the order
/// they are drawn in).
pub fn layout(state: &EditorState, camera: &Camera, eye: &GlobalTransform) -> Vec<Shown> {
    let mut wanted: Vec<(Target, Icon, egui::Color32, f32)> = Vec::new();
    for entry in state.editor.entries() {
        let icon = match entry.kind {
            Kind::Placeable => Icon::Square,
            Kind::Sound => Icon::Sound,
            Kind::Waypoint if is_post(&entry.tag) => Icon::Post,
            Kind::Waypoint => Icon::Waypoint,
            Kind::Trigger | Kind::Encounter => Icon::Alert,
            // A creature added since the scene was drawn is a dot until it
            // is drawn again.
            Kind::Creature => {
                creature_icon(state.creature_talks.get(&entry.template).copied().flatten())
            }
            Kind::Door | Kind::Store => Icon::Dot,
        };
        let target = Target::Object(entry.kind, entry.index);
        wanted.push((target, icon, colour(state::kind_colour(entry.kind).1), 1.0));
    }
    let kjk = &state.editor.project.kjk;
    for at in 0..kjk.props.len() {
        wanted.push((
            Target::Prop(at),
            Icon::Square,
            colour(state::PROP_MARKER.1),
            1.0,
        ));
    }
    for at in 0..kjk.lamps.len() {
        wanted.push((
            Target::Lamp(at),
            Icon::Light,
            colour(state::LAMP_MARKER.1),
            1.0,
        ));
    }
    // The sun, in the sky the way it stands from the level's middle.
    if state.editor.sun().is_some() {
        wanted.push((
            Target::Sun,
            Icon::Light,
            egui::Color32::from_rgb(255, 214, 90),
            1.6,
        ));
    }
    if state.editor.project.map.is_some() {
        wanted.push((
            Target::Entry,
            Icon::Post,
            colour(state::ENTRY_MARKER.1),
            1.3,
        ));
    }
    if state.show_lights {
        for (at, light) in kjk.lights.iter().enumerate() {
            let shown = colour(state::light_texture(light.colour).1);
            wanted.push((Target::Light(at), Icon::Light, shown, 1.0));
        }
        for (at, (light, _)) in state.editor.seen_lights.iter().enumerate() {
            let shown = colour(state::light_texture(light.colour).1);
            wanted.push((Target::Fitted(at), Icon::Light, shown, 0.6));
        }
    }

    let eye_at = eye.translation();
    let ahead = *eye.forward();
    let mut shown: Vec<Shown> = Vec::new();
    for (target, icon, tint, size) in wanted {
        let Some((mut position, _)) = state.editor.placement(target) else {
            continue;
        };
        if target == Target::Sun
            && let Some(sun) = state.editor.sun()
        {
            position = sun_marker(state.editor.level_middle, sun.at);
        }
        let world = game_to_bevy() * (Vec3::from_array(position) + Vec3::Z * rise(target));
        let distance = world.distance(eye_at);
        if distance > FARTHEST || (world - eye_at).dot(ahead) <= 0.0 {
            continue;
        }
        let Ok(at) = camera.world_to_viewport(eye, world) else {
            continue;
        };
        let selected = state.editor.selected == Some(target);
        let grown = if selected { 1.35 } else { 1.0 };
        let side = (360.0 / distance.max(0.1)).clamp(20.0, 32.0) * size * grown;
        // A far marker fades, so the near ones stand out.
        let faded = if selected {
            1.0
        } else {
            1.0 - (distance / FARTHEST).clamp(0.0, 1.0) * 0.6
        };
        shown.push(Shown {
            target,
            icon,
            at: egui::pos2(at.x, at.y),
            side,
            distance,
            colour: tint.gamma_multiply(faded),
        });
    }
    shown.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    shown.truncate(MOST);
    // The selection is always drawn, last, over the rest.
    shown.reverse();
    if let Some(at) = shown
        .iter()
        .position(|marker| state.editor.selected == Some(marker.target))
    {
        let selected = shown.remove(at);
        shown.push(selected);
    }
    shown
}

/// Draws the markers over the view: each icon on a dark disc so it shows
/// against any wall, the selected one ringed.
pub fn draw(painter: &egui::Painter, icons: &Icons, shown: &[Shown], selected: Option<Target>) {
    for marker in shown {
        if !painter.clip_rect().expand(marker.side).contains(marker.at) {
            continue;
        }
        let back =
            egui::Color32::from_black_alpha((150.0 * marker.colour.a() as f32 / 255.0) as u8);
        painter.circle_filled(marker.at, marker.side * 0.62, back);
        if selected == Some(marker.target) {
            painter.circle_stroke(
                marker.at,
                marker.side * 0.68,
                egui::Stroke::new(2.0, egui::Color32::YELLOW),
            );
        }
        let rect =
            egui::Rect::from_center_size(marker.at, egui::vec2(marker.side, marker.side) * 0.82);
        icons.paint(painter, marker.icon, rect, marker.colour);
    }
}

/// The marker under `point` (window points), the nearest one where markers
/// overlap.
pub fn under(shown: &[Shown], point: egui::Pos2) -> Option<Target> {
    (shown.iter())
        .filter(|marker| marker.at.distance(point) <= marker.side * 0.62)
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
        .map(|marker| marker.target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws() {
        for (icon, source) in SOURCES {
            let image = picture(source, 32).unwrap_or_else(|why| panic!("{icon:?}: {why}"));
            let inked = image.pixels.iter().filter(|pixel| pixel.a() > 0).count();
            assert!(inked > 20, "{icon:?} draws {inked} pixels");
        }
    }

    #[test]
    fn posts_are_told_by_their_tags() {
        assert!(is_post("dan13_wp_carth_halk"));
        assert!(is_post("POST_dan13_zzshari"));
        assert!(is_post("K_MESSENGER_SPAWN_PC0"));
        assert!(is_post("wp_hk47_01"));
        assert!(!is_post("WP_dan13_jedim_01"));
        assert!(!is_post("wp_hkfoo"));
    }
}
