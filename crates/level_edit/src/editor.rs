//! Edits made to a project, with undo and redo.
//!
//! Every edit keeps a copy of the project's GIT and kjk-only content as it
//! was before (`State`). Undo puts the copy back, and redo the state undo
//! left. An area's GIT is a few tens of kilobytes, so a copy for each edit
//! costs little next to the changes it keeps exact.

use crate::area::{CollisionBox, Prop, SCALES, SMALLEST_SIDE};
use crate::git::{self, Entry, Kind};
use crate::project::Project;

/// What an edit acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A GIT entry: its kind and its place in that kind's list.
    Object(Kind, usize),
    Prop(usize),
    Box(usize),
    /// A room of the project's new map (`map`).
    Room(usize),
    /// Where the player starts in the project's new map.
    Entry,
    /// A light placed by hand (`KjkArea::lights`).
    Light(usize),
    /// A light the game places itself at a fitting, as the view last drew
    /// it (`Editor::seen_lights`): shown and inspected, not changed.
    Fitted(usize),
}

/// One change to the project.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// To a position, metres.
    Move(Target, [f32; 3]),
    /// To a facing, degrees from +X turning left.
    Turn(Target, f32),
    /// A copy of it, standing where it stands.
    Duplicate(Target),
    Delete(Target),
    AddObject {
        kind: Kind,
        template: String,
        position: [f32; 3],
        facing: f32,
        /// The template's own fields, for a kind read whole from its entry
        /// (a waypoint, `git::add_from`).
        base: Option<fmt_odyssey::gff::GffStruct>,
    },
    AddProp(Prop),
    AddBox(CollisionBox),
    /// A box to a new size.
    Resize(usize, [f32; 3]),
    /// A prop to a new scale.
    Scale(usize, f32),
    /// A room of the install added to the project's new map.
    AddRoom(crate::map::Room),
    /// A box given another role (`area::BoxRole`).
    Role(usize, crate::area::BoxRole),
    /// A light placed by hand added.
    AddLight(crate::lights::PlacedLight),
    /// A light placed by hand changed (its colour, reach, cone, strip or
    /// reason; `Move` and `Turn` place and aim it).
    SetLight(usize, crate::lights::PlacedLight),
}

/// What a template change reads of the install: a template's bytes by
/// resref and type, and whether the install has a template of a name.
pub type Install<'a> = (
    &'a dyn Fn(&str, fmt_odyssey::ResType) -> Option<Vec<u8>>,
    &'a dyn Fn(&str) -> bool,
);

/// The kinds of GIT object the editor moves, turns, copies and deletes:
/// creatures, placeables and waypoints. Doors are left where their
/// walkmesh is, and triggers, sounds, encounters and stores are shown
/// only.
pub const EDITABLE: [Kind; 7] = [
    Kind::Creature,
    Kind::Placeable,
    Kind::Waypoint,
    Kind::Trigger,
    Kind::Sound,
    Kind::Encounter,
    Kind::Store,
];

/// The kinds `Edit::AddObject` makes, from a template of the install. An
/// encounter is left out: its spawn points are not made here.
pub const ADDABLE: [Kind; 6] = [
    Kind::Creature,
    Kind::Placeable,
    Kind::Waypoint,
    Kind::Trigger,
    Kind::Sound,
    Kind::Store,
];

#[derive(Debug, Clone)]
struct State {
    git: fmt_odyssey::gff::Gff,
    kjk: crate::area::KjkArea,
    templates: std::collections::BTreeMap<String, crate::template::Template>,
    map: Option<crate::map::NewMap>,
    selected: Option<Target>,
}

/// A project being edited.
pub struct Editor {
    pub project: Project,
    pub selected: Option<Target>,
    /// The lights the game places itself, as the view last drew them, each
    /// with where it comes from (`Target::Fitted`). Not part of the
    /// project.
    pub seen_lights: Vec<(crate::lights::PlacedLight, String)>,
    undo: Vec<State>,
    redo: Vec<State>,
    /// Counts every change of the project (edits, undos, redos), so a view
    /// knows when to draw it again.
    changes: u64,
    /// `changes` when it was last saved or opened.
    saved: u64,
}

impl Editor {
    pub fn new(project: Project) -> Editor {
        Editor {
            project,
            selected: None,
            seen_lights: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            changes: 0,
            saved: 0,
        }
    }

    /// The GIT's objects (`git::entries`).
    pub fn entries(&self) -> Vec<Entry> {
        git::entries(&self.project.git)
    }

    /// The GIT entry `kind` `index`, if there is one.
    pub fn entry(&self, kind: Kind, index: usize) -> Option<Entry> {
        self.entries()
            .into_iter()
            .find(|entry| entry.kind == kind && entry.index == index)
    }

    /// Where the target stands and how it faces (degrees), if it exists.
    pub fn placement(&self, target: Target) -> Option<([f32; 3], f32)> {
        match target {
            Target::Object(kind, index) => self
                .entry(kind, index)
                .map(|entry| (entry.position, entry.facing)),
            Target::Prop(at) => self
                .project
                .kjk
                .props
                .get(at)
                .map(|prop| (prop.position, prop.yaw)),
            Target::Box(at) => self
                .project
                .kjk
                .boxes
                .get(at)
                .map(|shape| (shape.centre, shape.yaw)),
            Target::Room(at) => self
                .project
                .map
                .as_ref()
                .and_then(|map| map.rooms.get(at))
                .map(|room| (room.position, 0.0)),
            Target::Entry => self
                .project
                .map
                .as_ref()
                .map(|map| (map.entry, map.entry_facing)),
            Target::Light(at) => self
                .project
                .kjk
                .lights
                .get(at)
                .map(|light| (light.at, aim_facing(light.aim))),
            Target::Fitted(at) => self
                .seen_lights
                .get(at)
                .map(|(light, _)| (light.at, aim_facing(light.aim))),
        }
    }

    /// How many changes there have been since the editor opened.
    pub fn changes(&self) -> u64 {
        self.changes
    }

    /// Whether there are changes the folder does not hold yet.
    pub fn unsaved(&self) -> bool {
        self.changes != self.saved
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    fn state(&self) -> State {
        State {
            git: self.project.git.clone(),
            kjk: self.project.kjk.clone(),
            templates: self.project.templates.clone(),
            map: self.project.map.clone(),
            selected: self.selected,
        }
    }

    fn restore(&mut self, state: State) {
        self.project.git = state.git;
        self.project.kjk = state.kjk;
        self.project.templates = state.templates;
        self.project.map = state.map;
        self.selected = state.selected;
        self.changes += 1;
    }

    /// Makes `edit`, kept for undo. Refused, with nothing changed, when its
    /// target does not exist or the edit does not apply to it. The edited
    /// or added thing is selected afterwards; a delete selects nothing.
    pub fn apply(&mut self, edit: Edit) -> Result<(), String> {
        let before = self.state();
        match self.change(edit) {
            Ok(selected) => {
                self.undo.push(before);
                self.redo.clear();
                self.selected = selected;
                self.changes += 1;
                Ok(())
            }
            Err(why) => {
                self.restore_quietly(before);
                Err(why)
            }
        }
    }

    fn restore_quietly(&mut self, state: State) {
        self.project.git = state.git;
        self.project.kjk = state.kjk;
        self.project.templates = state.templates;
        self.project.map = state.map;
        self.selected = state.selected;
    }

    /// How many GIT objects use the template `resref`.
    pub fn users(&self, resref: &str) -> usize {
        self.entries()
            .iter()
            .filter(|entry| entry.template == resref)
            .count()
    }

    /// Makes `change` in the object `target`, kept for undo as one edit: in
    /// its GIT entry for a kind read whole from it (a door, a waypoint; a
    /// trigger's tag too), else in its template (`template`). An object made from a template of the
    /// install, or from one of the project's that other objects use too,
    /// first gets a copy of its own: `read` gives a template's bytes as the
    /// install has it, and `taken` says whether the install has a template
    /// of a name, so a copy's name is new. Only placeables and creatures
    /// are changed.
    pub fn change_template(
        &mut self,
        target: Target,
        change: &crate::template::Change,
        (read, taken): Install<'_>,
    ) -> Result<(), String> {
        let before = self.state();
        match self.change_template_in(target, change, (read, taken)) {
            Ok(()) => {
                self.undo.push(before);
                self.redo.clear();
                self.selected = Some(target);
                self.changes += 1;
                Ok(())
            }
            Err(why) => {
                self.restore_quietly(before);
                Err(why)
            }
        }
    }

    fn change_template_in(
        &mut self,
        target: Target,
        change: &crate::template::Change,
        (read, taken): Install<'_>,
    ) -> Result<(), String> {
        use crate::template::{self, Template};
        let Target::Object(kind, index) = target else {
            return Err("only an object of the area has a template".into());
        };
        let entry = self
            .entry(kind, index)
            .ok_or_else(|| format!("{target:?} does not exist"))?;
        // A door's and a waypoint's tag and scripts, and a trigger's tag,
        // are their GIT entry's.
        let in_entry = match change {
            template::Change::Tag(_) => template::tag_in_entry(kind),
            _ => template::whole(kind),
        };
        if in_entry {
            let item = git::item(&self.project.git, kind, index)
                .ok_or_else(|| format!("{target:?} does not exist"))?;
            let mut item = item.clone();
            template::apply_entry(&mut item, change)?;
            for field in item.fields {
                let label = String::from_utf8_lossy(&field.label).into_owned();
                git::set_entry_field(&mut self.project.git, kind, index, &label, field.value);
            }
            return Ok(());
        }
        let restype = template::restype(kind).ok_or("this object has no template")?;
        let own = self.project.templates.contains_key(&entry.template)
            && self.users(&entry.template) == 1;
        let resref = if own {
            entry.template.clone()
        } else {
            let (gff, from) = match self.project.templates.get(&entry.template) {
                Some(made) => (made.gff.clone(), made.from.clone()),
                None => {
                    let bytes = read(&entry.template, restype)
                        .ok_or(format!("the template {} does not read", entry.template))?;
                    let gff = fmt_odyssey::gff::Gff::parse(&bytes)
                        .map_err(|error| format!("{}: {error}", entry.template))?;
                    (gff, entry.template.clone())
                }
            };
            let templates = &self.project.templates;
            let name =
                template::fresh_name(&from, |name| templates.contains_key(name) || taken(name));
            self.project
                .templates
                .insert(name.clone(), Template { restype, from, gff });
            git::set_template(&mut self.project.git, kind, index, &name);
            name
        };
        let made = self
            .project
            .templates
            .get_mut(&resref)
            .expect("made above or found");
        template::apply(&mut made.gff, restype, change)?;
        // A copy no object uses any more is dropped.
        let used: std::collections::BTreeSet<String> = self
            .entries()
            .into_iter()
            .map(|entry| entry.template)
            .collect();
        self.project.templates.retain(|name, _| used.contains(name));
        Ok(())
    }

    fn editable(target: Target) -> Result<(), String> {
        match target {
            Target::Fitted(_) => Err(
                "the game places this light at a fitting; copy it as a placed light to change it"
                    .into(),
            ),
            Target::Object(kind, _) if !EDITABLE.contains(&kind) => Err(format!(
                "a {kind:?} cannot be changed in the editor: only creatures, placeables and waypoints"
            )),
            _ => Ok(()),
        }
    }

    fn change(&mut self, edit: Edit) -> Result<Option<Target>, String> {
        let missing = |target: Target| format!("{target:?} does not exist");
        let kjk = &mut self.project.kjk;
        let map = &mut self.project.map;

        match edit {
            Edit::Move(target, to) => {
                Self::editable(target)?;
                if !to.iter().all(|value| value.is_finite()) {
                    return Err("a position must be finite".into());
                }
                let moved = match target {
                    Target::Object(kind, index) => {
                        git::set_position(&mut self.project.git, kind, index, to)
                    }
                    Target::Prop(at) => kjk.props.get_mut(at).map(|p| p.position = to).is_some(),
                    Target::Box(at) => kjk.boxes.get_mut(at).map(|b| b.centre = to).is_some(),
                    Target::Room(at) => rooms(map)?
                        .get_mut(at)
                        .map(|room| room.position = to)
                        .is_some(),
                    Target::Entry => map.as_mut().map(|map| map.entry = to).is_some(),
                    Target::Light(at) => kjk.lights.get_mut(at).map(|l| l.at = to).is_some(),
                    Target::Fitted(_) => false,
                };
                moved.then_some(Some(target)).ok_or_else(|| missing(target))
            }
            Edit::Turn(target, degrees) => {
                Self::editable(target)?;
                if let Target::Object(Kind::Trigger | Kind::Encounter, _) = target {
                    return Err("a trigger's or an encounter's outline does not turn".into());
                }
                if let Target::Room(_) = target {
                    return Err("a room does not turn: the layout places rooms unturned".into());
                }
                if !degrees.is_finite() {
                    return Err("a facing must be finite".into());
                }
                let degrees = degrees.rem_euclid(360.0);
                let turned = match target {
                    Target::Object(kind, index) => {
                        git::set_facing(&mut self.project.git, kind, index, degrees)
                    }
                    Target::Prop(at) => kjk.props.get_mut(at).map(|p| p.yaw = degrees).is_some(),
                    Target::Box(at) => kjk.boxes.get_mut(at).map(|b| b.yaw = degrees).is_some(),
                    Target::Room(_) | Target::Fitted(_) => false,
                    Target::Entry => map.as_mut().map(|map| map.entry_facing = degrees).is_some(),
                    // A light's aim turns about Z, its height kept; one that
                    // shines all ways or straight down takes a level aim.
                    Target::Light(at) => kjk
                        .lights
                        .get_mut(at)
                        .map(|light| {
                            let (sin, cos) = degrees.to_radians().sin_cos();
                            let flat = (light.aim[0].powi(2) + light.aim[1].powi(2)).sqrt();
                            let flat = if flat > 0.01 { flat } else { 1.0 };
                            light.aim = [cos * flat, sin * flat, light.aim[2]];
                        })
                        .is_some(),
                };
                turned
                    .then_some(Some(target))
                    .ok_or_else(|| missing(target))
            }
            Edit::Duplicate(target) => {
                Self::editable(target)?;
                match target {
                    Target::Object(kind, index) => {
                        git::duplicate(&mut self.project.git, kind, index)
                            .map(|copy| Some(Target::Object(kind, copy)))
                            .ok_or_else(|| missing(target))
                    }
                    Target::Prop(at) => {
                        let mut copy = kjk.props.get(at).ok_or_else(|| missing(target))?.clone();
                        copy.name = kjk.free_name("prop");
                        kjk.props.push(copy);
                        Ok(Some(Target::Prop(kjk.props.len() - 1)))
                    }
                    Target::Box(at) => {
                        let mut copy = kjk.boxes.get(at).ok_or_else(|| missing(target))?.clone();
                        copy.name = kjk.free_name("box");
                        kjk.boxes.push(copy);
                        Ok(Some(Target::Box(kjk.boxes.len() - 1)))
                    }
                    Target::Room(at) => {
                        let rooms = rooms(map)?;
                        let copy = rooms.get(at).ok_or_else(|| missing(target))?.clone();
                        rooms.push(copy);
                        Ok(Some(Target::Room(rooms.len() - 1)))
                    }
                    Target::Entry => Err("a map has one entry".into()),
                    Target::Light(at) => {
                        let copy = kjk.lights.get(at).ok_or_else(|| missing(target))?.clone();
                        kjk.lights.push(copy);
                        Ok(Some(Target::Light(kjk.lights.len() - 1)))
                    }
                    Target::Fitted(_) => Err("a fitted light is not copied here".into()),
                }
            }
            Edit::Delete(target) => {
                Self::editable(target)?;
                let gone = match target {
                    Target::Object(kind, index) => git::remove(&mut self.project.git, kind, index),
                    Target::Prop(at) => (at < kjk.props.len())
                        .then(|| kjk.props.remove(at))
                        .is_some(),
                    Target::Box(at) => (at < kjk.boxes.len())
                        .then(|| kjk.boxes.remove(at))
                        .is_some(),
                    Target::Room(at) => {
                        let rooms = rooms(map)?;
                        (at < rooms.len()).then(|| rooms.remove(at)).is_some()
                    }
                    Target::Entry => return Err("a map's entry is moved, not deleted".into()),
                    Target::Light(at) => (at < kjk.lights.len())
                        .then(|| kjk.lights.remove(at))
                        .is_some(),
                    Target::Fitted(_) => false,
                };
                gone.then_some(None).ok_or_else(|| missing(target))
            }
            Edit::AddObject {
                kind,
                template,
                position,
                facing,
                base,
            } => {
                if !ADDABLE.contains(&kind) {
                    return Err(format!(
                        "a {kind:?} cannot be added: only creatures and placeables"
                    ));
                }
                if template.is_empty() || template.len() > git::RESREF_LEN {
                    return Err(format!("{template:?} is not a template's name"));
                }
                let index = git::add_from(
                    &mut self.project.git,
                    kind,
                    (&template, base.as_ref()),
                    position,
                    facing,
                );
                Ok(Some(Target::Object(kind, index)))
            }
            Edit::AddProp(mut prop) => {
                if prop.model.is_empty() {
                    return Err("a prop needs a model".into());
                }
                if prop.name.is_empty() || kjk.props.iter().any(|p| p.name == prop.name) {
                    prop.name = kjk.free_name("prop");
                }
                kjk.props.push(prop);
                Ok(Some(Target::Prop(kjk.props.len() - 1)))
            }
            Edit::AddBox(mut shape) => {
                if shape.size.iter().any(|side| *side < SMALLEST_SIDE) {
                    return Err(format!("a box's sides must be {SMALLEST_SIDE} m or more"));
                }
                if shape.name.is_empty() || kjk.boxes.iter().any(|b| b.name == shape.name) {
                    shape.name = kjk.free_name("box");
                }
                kjk.boxes.push(shape);
                Ok(Some(Target::Box(kjk.boxes.len() - 1)))
            }
            Edit::Resize(at, size) => {
                if size
                    .iter()
                    .any(|side| !side.is_finite() || *side < SMALLEST_SIDE)
                {
                    return Err(format!("a box's sides must be {SMALLEST_SIDE} m or more"));
                }
                let shape = kjk
                    .boxes
                    .get_mut(at)
                    .ok_or_else(|| missing(Target::Box(at)))?;
                shape.size = size;
                Ok(Some(Target::Box(at)))
            }
            Edit::Scale(at, scale) => {
                if !(SCALES.0..=SCALES.1).contains(&scale) {
                    return Err(format!(
                        "a prop's scale must be from {} to {}",
                        SCALES.0, SCALES.1
                    ));
                }
                let prop = kjk
                    .props
                    .get_mut(at)
                    .ok_or_else(|| missing(Target::Prop(at)))?;
                prop.scale = scale;
                Ok(Some(Target::Prop(at)))
            }
            Edit::AddLight(light) => {
                light_problem(&light)?;
                kjk.lights.push(light);
                Ok(Some(Target::Light(kjk.lights.len() - 1)))
            }
            Edit::SetLight(at, light) => {
                light_problem(&light)?;
                let slot = kjk
                    .lights
                    .get_mut(at)
                    .ok_or_else(|| missing(Target::Light(at)))?;
                *slot = light;
                Ok(Some(Target::Light(at)))
            }
            Edit::Role(at, role) => {
                let shape = kjk
                    .boxes
                    .get_mut(at)
                    .ok_or_else(|| missing(Target::Box(at)))?;
                shape.role = role;
                Ok(Some(Target::Box(at)))
            }
            Edit::AddRoom(room) => {
                if room.model.is_empty() || room.model.len() > git::RESREF_LEN {
                    return Err(format!("{:?} is not a room's model", room.model));
                }
                if !room
                    .position
                    .iter()
                    .chain(&room.from)
                    .all(|value| value.is_finite())
                {
                    return Err("a position must be finite".into());
                }
                let rooms = rooms(map)?;
                rooms.push(room);
                Ok(Some(Target::Room(rooms.len() - 1)))
            }
        }
    }

    /// Makes `edit` as a part of the last one: a drag of a gizmo or of a
    /// number makes many edits, which one undo takes back together. Without
    /// an edit before it, it is a new edit (`apply`).
    pub fn apply_continuing(&mut self, edit: Edit) -> Result<(), String> {
        if self.undo.is_empty() {
            return self.apply(edit);
        }
        let before = self.state();
        match self.change(edit) {
            Ok(selected) => {
                self.redo.clear();
                self.selected = selected;
                self.changes += 1;
                Ok(())
            }
            Err(why) => {
                self.restore_quietly(before);
                Err(why)
            }
        }
    }

    /// Takes back the last edit. False when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(state) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.state());
        self.restore(state);
        true
    }

    /// Makes the last edit undone again. False when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(state) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.state());
        self.restore(state);
        true
    }

    /// Selects the first GIT object whose tag is `tag` (in any case). The
    /// entry's own tag is used, else the one `template_tag` gives for its
    /// template. False when no object has it.
    pub fn select_tag(&mut self, tag: &str, template_tag: impl Fn(&Entry) -> String) -> bool {
        let found = self.entries().into_iter().find(|entry| {
            let own = if entry.tag.is_empty() {
                template_tag(entry)
            } else {
                entry.tag.clone()
            };
            own.eq_ignore_ascii_case(tag)
        });
        match found {
            Some(entry) => {
                self.selected = Some(Target::Object(entry.kind, entry.index));
                true
            }
            None => false,
        }
    }

    /// Writes the project's folder (`Project::save`).
    pub fn save(&mut self, made_by: &str) -> Result<Vec<std::path::PathBuf>, String> {
        let written = self.project.save(made_by)?;
        self.saved = self.changes;
        Ok(written)
    }

    /// Writes the project's folder and, for a new map, its module
    /// (`Project::save_module`), whose rooms `read` gives as the install
    /// has them.
    pub fn save_with(
        &mut self,
        made_by: &str,
        read: &dyn Fn(&str, fmt_odyssey::ResType) -> Option<Vec<u8>>,
    ) -> Result<Vec<std::path::PathBuf>, String> {
        let mut written = self.project.save(made_by)?;
        written.extend(self.project.save_module(read)?);
        self.saved = self.changes;
        Ok(written)
    }
}

/// The rooms of the project's new map; refused for a project that makes
/// none.
fn rooms(map: &mut Option<crate::map::NewMap>) -> Result<&mut Vec<crate::map::Room>, String> {
    map.as_mut()
        .map(|map| &mut map.rooms)
        .ok_or_else(|| "the project makes no new map".to_string())
}

/// The facing (degrees from +X) of a light's aim seen from above; 0 for a
/// light that shines all ways or straight down.
fn aim_facing(aim: [f32; 3]) -> f32 {
    if aim[0].abs() < 1e-4 && aim[1].abs() < 1e-4 {
        return 0.0;
    }
    aim[1].atan2(aim[0]).to_degrees()
}

/// What is wrong with a light placed by hand, if anything.
fn light_problem(light: &crate::lights::PlacedLight) -> Result<(), String> {
    let numbers = light
        .at
        .iter()
        .chain(&light.colour)
        .chain(&light.aim)
        .chain(&light.way)
        .chain([&light.radius, &light.cone, &light.half]);
    if !numbers.copied().all(f32::is_finite) {
        return Err("a light's numbers must be finite".into());
    }
    if light.radius <= 0.0 {
        return Err("a light must reach some way".into());
    }
    if light.colour.iter().any(|value| *value < 0.0) {
        return Err("a light's colour cannot be below 0".into());
    }
    Ok(())
}
