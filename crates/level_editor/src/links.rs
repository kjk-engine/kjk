//! The links between the level's objects that their scripts make
//! (`level_edit::links`), as the editor shows them (`docs/LEVEL-EDIT.md`,
//! "What an object's scripts name"):
//!
//! - under the selected object's behaviour, the objects its scripts name
//!   and the objects whose scripts name it, each a click away (`section`);
//! - over the view, a line from the selected object to each of them, with
//!   an arrow from the object that runs the script to the one it names
//!   (`in_view`);
//! - in the conversation window, the objects each line's script names
//!   (`script_names`);
//! - over the agent link, the same under an object's details
//!   (`described`).
//!
//! The links are found again after each change of the project and each
//! compiling of its scripts (`table`). They are read from the compiled
//! scripts, so a script written and not yet compiled shows the links of
//! its last compiling.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::egui;
use fmt_odyssey::ResType;
use level_edit::links::{Link, Object, Run};
use level_edit::{Kind, Target};

use kjk_game::scene::game_to_bevy;

use crate::state::{self, EditorState};

/// The level's links, as found for one state of the project.
#[derive(Default)]
pub struct Table {
    /// Each object of the level, in the order of `Editor::entries`: its
    /// kind, its place in its kind's list, and its tag.
    pub objects: Vec<(Kind, usize, String)>,
    /// The links, each between two places in `objects`.
    pub links: Vec<Link>,
}

impl Table {
    fn place(&self, kind: Kind, index: usize) -> Option<usize> {
        (self.objects.iter()).position(|(had, at, _)| *had == kind && *at == index)
    }

    /// The links from the object: what its scripts name.
    pub fn names(&self, kind: Kind, index: usize) -> Vec<&Link> {
        let here = self.place(kind, index);
        (self.links.iter())
            .filter(|link| Some(link.from) == here)
            .collect()
    }

    /// The links to the object: whose scripts name it.
    pub fn named_by(&self, kind: Kind, index: usize) -> Vec<&Link> {
        let here = self.place(kind, index);
        (self.links.iter())
            .filter(|link| Some(link.to) == here)
            .collect()
    }
}

/// What the editor keeps of the links between two changes.
#[derive(Default)]
pub struct Kept {
    /// The project's count of changes and the round of compiling that
    /// `table` was found for.
    at: Option<(u64, u32)>,
    table: Arc<Table>,
    /// The strings of each compiled script read so far, for the round of
    /// compiling `strings_round`.
    strings: BTreeMap<String, Arc<[String]>>,
    strings_round: u32,
    /// The scripts of each of the game's conversations read so far. The
    /// install does not change while the editor is open.
    talks: BTreeMap<String, Arc<Vec<Run>>>,
    /// The tags of the level each script names (`script_names`), for
    /// `table`.
    named: BTreeMap<String, Arc<Vec<String>>>,
}

/// The strings of the compiled script `name`: the project's own, else the
/// install's.
fn strings_of(state: &EditorState, name: &str) -> Arc<[String]> {
    let own = std::fs::read(state.editor.project.folder.join(format!("{name}.ncs"))).ok();
    let bytes = own.or_else(|| state.setup.resolver.read(name, ResType::NCS));
    let strings = bytes.map_or_else(Vec::new, |bytes| level_edit::links::strings(&bytes));
    Arc::from(strings)
}

/// The scripts of the conversation `name`: the project's as it stands,
/// else the game's, read once.
fn talk_runs(
    state: &EditorState,
    name: &str,
    games: &mut BTreeMap<String, Arc<Vec<Run>>>,
) -> Arc<Vec<Run>> {
    if let Some(own) = state.editor.project.conversations.get(name) {
        let dlg = level_edit::dialog::read(own);
        return Arc::new(level_edit::links::of_conversation(name, &dlg));
    }
    games
        .entry(name.to_string())
        .or_insert_with(|| {
            let dlg = (state.setup.resolver.read(name, ResType::DLG))
                .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                .map(|gff| level_edit::dialog::read(&gff));
            Arc::new(dlg.map_or_else(Vec::new, |dlg| {
                level_edit::links::of_conversation(name, &dlg)
            }))
        })
        .clone()
}

/// The level's links as the project stands now.
pub(crate) fn table(state: &EditorState) -> Arc<Table> {
    let key = (state.editor.changes(), state.script_round);
    let Ok(mut kept) = state.links.lock() else {
        return Arc::default();
    };
    if kept.at == Some(key) {
        return kept.table.clone();
    }
    if kept.strings_round != state.script_round {
        kept.strings.clear();
        kept.strings_round = state.script_round;
    }
    let mut games = std::mem::take(&mut kept.talks);
    // What says what an object does is read once for all the objects of
    // one template, and a conversation once for all that speak it.
    let mut of_template: BTreeMap<(Kind, String), (String, Vec<Run>)> = BTreeMap::new();
    let mut of_talk: BTreeMap<String, Arc<Vec<Run>>> = BTreeMap::new();
    let mut listed: Vec<(Kind, usize, String)> = Vec::new();
    let mut objects: Vec<Object> = Vec::new();
    for entry in state.editor.entries() {
        let mut read = || {
            let Some(gff) = state.behaviour_source(entry.kind, &entry) else {
                return (String::new(), Vec::new());
            };
            let mut runs = level_edit::links::of_events(&gff);
            let talk = (gff.root.resref("Conversation"))
                .map(|name| name.to_ascii_lowercase())
                .filter(|name| !name.is_empty());
            if let Some(name) = talk {
                let spoken = of_talk
                    .entry(name.clone())
                    .or_insert_with(|| talk_runs(state, &name, &mut games));
                runs.extend(spoken.iter().cloned());
            }
            (state.tag_of(&entry), runs)
        };
        // A door and a waypoint are read whole from their own entries.
        let (tag, runs) = if level_edit::template::restype(entry.kind).is_some() {
            of_template
                .entry((entry.kind, entry.template.clone()))
                .or_insert_with(read)
                .clone()
        } else {
            read()
        };
        // The entry's own tag, where it has one, names the object.
        let tag = if entry.tag.is_empty() {
            tag
        } else {
            entry.tag.clone()
        };
        listed.push((entry.kind, entry.index, tag.clone()));
        objects.push(Object { tag, runs });
    }
    let mut strings = std::mem::take(&mut kept.strings);
    let links = level_edit::links::links(&objects, &mut |script| {
        strings
            .entry(script.to_string())
            .or_insert_with(|| strings_of(state, script))
            .clone()
    });
    kept.strings = strings;
    kept.talks = games;
    kept.named.clear();
    kept.table = Arc::new(Table {
        objects: listed,
        links,
    });
    kept.at = Some(key);
    kept.table.clone()
}

/// The tags of the level's objects that the compiled script `script`
/// names.
pub(crate) fn script_names(state: &EditorState, script: &str) -> Arc<Vec<String>> {
    let table = table(state);
    let Ok(mut kept) = state.links.lock() else {
        return Arc::default();
    };
    if let Some(named) = kept.named.get(script) {
        return named.clone();
    }
    let strings = kept
        .strings
        .entry(script.to_string())
        .or_insert_with(|| strings_of(state, script))
        .clone();
    let tags = table.objects.iter().map(|(_, _, tag)| tag.as_str());
    let named = Arc::new(level_edit::links::named(&strings, tags));
    kept.named.insert(script.to_string(), named.clone());
    named
}

/// The most links of one way the details list.
const MOST_ROWS: usize = 12;

/// The line and arrow of a link from the selected object to one its
/// scripts name.
const NAMES: egui::Color32 = egui::Color32::from_rgb(110, 220, 255);
/// The line and arrow of a link to the selected object from one whose
/// scripts name it.
const NAMED_BY: egui::Color32 = egui::Color32::from_rgb(255, 170, 80);

/// Under the selected object's behaviour: the objects its scripts name and
/// the objects whose scripts name it. A click on one selects it.
pub(crate) fn section(ui: &mut egui::Ui, state: &mut EditorState, (kind, index): (Kind, usize)) {
    let table = table(state);
    let ways = [
        (
            "This object's scripts name:",
            table.names(kind, index),
            NAMES,
            true,
        ),
        (
            "Other objects' scripts name this object:",
            table.named_by(kind, index),
            NAMED_BY,
            false,
        ),
    ];
    if ways.iter().all(|(_, links, _, _)| links.is_empty()) {
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new("Linked objects").strong());
    ui.label(
        egui::RichText::new(
            "A script finds an object by its tag. These links are read from the compiled scripts. \
             The view joins this object to each linked object with a line.",
        )
        .small(),
    );
    for (heading, links, colour, outward) in ways {
        if links.is_empty() {
            continue;
        }
        ui.label(egui::RichText::new(heading).color(colour));
        for link in links.iter().take(MOST_ROWS) {
            let other = if outward { link.to } else { link.from };
            let Some((kind, index, tag)) = table.objects.get(other) else {
                continue;
            };
            let word = state::kind_name(*kind);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .link(format!("{tag} ({word} {index})"))
                    .on_hover_text("Selects this object.")
                    .clicked()
                {
                    state.pending_command = Some(format!("select {word} {index}"));
                }
                ui.label(
                    egui::RichText::new(format!("{} runs {}", link.run.at, link.run.script))
                        .small(),
                );
            });
        }
        if links.len() > MOST_ROWS {
            ui.label(egui::RichText::new(format!("and {} more", links.len() - MOST_ROWS)).small());
        }
    }
}

/// An object's links for the agent link and a dump: what its scripts name
/// and what names it, each with the object, its tag, where the script
/// hangs and the script.
pub(crate) fn described(state: &EditorState, kind: Kind, index: usize) -> serde_json::Value {
    let table = table(state);
    let listed = |links: Vec<&Link>, outward: bool| -> Vec<serde_json::Value> {
        links
            .into_iter()
            .filter_map(|link| {
                let other = if outward { link.to } else { link.from };
                let (kind, index, tag) = table.objects.get(other)?;
                Some(serde_json::json!({
                    "object": format!("{} {index}", state::kind_name(*kind)),
                    "tag": tag,
                    "at": link.run.at,
                    "script": link.run.script,
                }))
            })
            .collect()
    };
    serde_json::json!({
        "names": listed(table.names(kind, index), true),
        "named_by": listed(table.named_by(kind, index), false),
    })
}

/// How near the camera (metres ahead of it) a link's line is cut.
const NEAR: f32 = 0.2;
/// How far from a marker's middle a link's line ends, and how wide the
/// ring round a linked object's marker is (points).
const RING: f32 = 20.0;
/// How long and how wide a link's arrow is (points).
const ARROW: (f32, f32) = (11.0, 5.0);

/// The line between two places of the world on the screen, cut where it
/// passes behind the camera, and whether each end is the place itself.
fn on_screen(
    camera: &Camera,
    eye: &GlobalTransform,
    (a, b): (Vec3, Vec3),
) -> Option<((egui::Pos2, bool), (egui::Pos2, bool))> {
    let ahead = |point: Vec3| (point - eye.translation()).dot(*eye.forward());
    let (ahead_a, ahead_b) = (ahead(a), ahead(b));
    let cut = |behind: Vec3, at: f32, front: Vec3, front_at: f32| {
        behind + (front - behind) * ((NEAR - at) / (front_at - at))
    };
    let (shown_a, shown_b) = match (ahead_a < NEAR, ahead_b < NEAR) {
        (true, true) => return None,
        (true, false) => (cut(a, ahead_a, b, ahead_b), b),
        (false, true) => (a, cut(b, ahead_b, a, ahead_a)),
        (false, false) => (a, b),
    };
    let to_screen =
        |point: Vec3| (camera.world_to_viewport(eye, point).ok()).map(|at| egui::pos2(at.x, at.y));
    Some((
        (to_screen(shown_a)?, ahead_a >= NEAR),
        (to_screen(shown_b)?, ahead_b >= NEAR),
    ))
}

/// Where a target's marker stands in the world.
fn marker_at(state: &EditorState, target: Target) -> Option<Vec3> {
    let (position, _) = state.editor.placement(target)?;
    Some(game_to_bevy() * (Vec3::from_array(position) + Vec3::Z * crate::markers::rise(target)))
}

/// Over the view: a line from the selected object to each object its
/// scripts name, and to each object whose scripts name it, with a ring
/// round the other object's marker. The arrow points from the object that
/// runs the script to the one it names. Drawn each frame, so the lines
/// follow an object while it is dragged.
pub(crate) fn in_view(
    painter: &egui::Painter,
    state: &EditorState,
    camera: &Camera,
    eye: &GlobalTransform,
) {
    let Some(Target::Object(kind, index)) = state.editor.selected else {
        return;
    };
    let table = table(state);
    let Some(here) = table.place(kind, index) else {
        return;
    };
    // Each other object once: whether this one names it, and whether it
    // names this one.
    let mut others: BTreeMap<usize, (bool, bool)> = BTreeMap::new();
    for link in &table.links {
        if link.from == here {
            others.entry(link.to).or_default().0 = true;
        } else if link.to == here {
            others.entry(link.from).or_default().1 = true;
        }
    }
    let Some(from) = marker_at(state, Target::Object(kind, index)) else {
        return;
    };
    for (other, (names, named_by)) in others {
        let Some((kind, index, _)) = table.objects.get(other) else {
            continue;
        };
        let Some(to) = marker_at(state, Target::Object(*kind, *index)) else {
            continue;
        };
        let Some(((here_at, here_whole), (there_at, there_whole))) =
            on_screen(camera, eye, (from, to))
        else {
            continue;
        };
        let colour = if names { NAMES } else { NAMED_BY };
        let along = there_at - here_at;
        let length = along.length();
        if length < 1.0 {
            continue;
        }
        let way = along / length;
        // The line stops at each marker's edge, where that marker shows.
        let gap = |whole: bool| if whole { RING.min(length * 0.5) } else { 0.0 };
        let start = here_at + way * gap(here_whole);
        let end = there_at - way * gap(there_whole);
        painter.line_segment([start, end], egui::Stroke::new(1.5, colour));
        let arrow = |tip: egui::Pos2, way: egui::Vec2, colour: egui::Color32| {
            let side = egui::vec2(-way.y, way.x) * ARROW.1;
            let back = tip - way * ARROW.0;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, back + side, back - side],
                colour,
                egui::Stroke::NONE,
            ));
        };
        if names && there_whole {
            arrow(end, way, NAMES);
        }
        if named_by && here_whole {
            arrow(start, -way, NAMED_BY);
        }
        if there_whole {
            painter.circle_stroke(there_at, RING, egui::Stroke::new(2.0, colour));
        }
    }
}
