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
    /// A lamp placed by hand (`KjkArea::lamps`).
    Lamp(usize),
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
    /// Objects copied (`clip::Clip`), put with their middle at `at`; the
    /// install's template names (`taken`) a pasted template keeps clear
    /// of.
    Paste {
        clip: Box<crate::clip::Clip>,
        at: [f32; 3],
        taken: Vec<String>,
    },
    /// A link between floors that walkers jump or drop across
    /// (`area::NavLink`).
    AddLink(crate::area::NavLink),
    /// The link at this place taken out.
    RemoveLink(usize),
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
    /// A kind of light fitting changed (`kinds`): every lamp of it, in
    /// every level of the game.
    SetKind(crate::kinds::KindEdit),
    /// The level a room of the bake aims at, or (`None`) none.
    SetLevel(String, Option<f32>),
    /// A light placed by hand changed (its colour, reach, cone, strip or
    /// reason; `Move` and `Turn` place and aim it).
    SetLight(usize, crate::lights::PlacedLight),
    /// A lamp placed by hand added (`lamps`).
    AddLamp(crate::lamps::PlacedLamp),
    /// A lamp placed by hand changed (its model, mesh, kind or reason;
    /// `Move` and `Turn` place and turn it).
    SetLamp(usize, crate::lamps::PlacedLamp),
    /// A door or trigger of another module's area (`Project::others`) led
    /// elsewhere: the area of `module`, its GIT as the install has it
    /// (`base`, read when the project does not hold that area yet), the
    /// object, and where it leads.
    LinkOther {
        module: String,
        area: String,
        base: Option<fmt_odyssey::gff::Gff>,
        kind: Kind,
        index: usize,
        link: crate::template::Link,
    },
    /// Another module's area given back to the install: the project no
    /// longer changes its doors and triggers.
    DropOther(String),
    /// A conversation of the project made: `base` is a copy of the game's
    /// of that name, else it starts empty (`dialog::empty`).
    NewConversation {
        name: String,
        base: Option<fmt_odyssey::gff::Gff>,
    },
    /// A change to one of the project's conversations (`dialog::Edit`).
    Conversation(String, crate::dialog::Edit),
    /// A conversation taken out of the project.
    DropConversation(String),
    /// A trigger's or an encounter's outline: its points where they are in
    /// the area (metres), three or more, in order round it.
    Outline(Target, Vec<[f32; 3]>),
    /// An encounter's spawn points: where each is in the area (metres) and
    /// the way it faces (radians).
    SpawnPoints(Target, Vec<([f32; 3], f32)>),
    /// The area's settings (`settings`). `base` is the install's ARE, for
    /// the first change of an installed area's ARE.
    Area {
        change: crate::settings::AreaChange,
        base: Option<fmt_odyssey::gff::Gff>,
    },
}

/// What a template change reads of the install: a template's bytes by
/// resref and type, and whether the install has a template of a name.
pub type Install<'a> = (
    &'a dyn Fn(&str, fmt_odyssey::ResType) -> Option<Vec<u8>>,
    &'a dyn Fn(&str) -> bool,
);

/// The kinds of GIT object the editor moves, turns, copies and deletes:
/// every kind. An installed door stands in its room's doorway; moved, its
/// walkmesh goes with it and the doorway is left open, which is the
/// user's call.
pub const EDITABLE: [Kind; 8] = [
    Kind::Creature,
    Kind::Placeable,
    Kind::Door,
    Kind::Waypoint,
    Kind::Trigger,
    Kind::Sound,
    Kind::Encounter,
    Kind::Store,
];

/// The kinds `Edit::AddObject` makes, from a template of the install.
pub const ADDABLE: [Kind; 8] = [
    Kind::Creature,
    Kind::Placeable,
    Kind::Door,
    Kind::Waypoint,
    Kind::Trigger,
    Kind::Sound,
    Kind::Store,
    Kind::Encounter,
];

#[derive(Debug, Clone)]
struct State {
    git: fmt_odyssey::gff::Gff,
    kjk: crate::area::KjkArea,
    templates: std::collections::BTreeMap<String, crate::template::Template>,
    map: Option<crate::map::NewMap>,
    kinds: Vec<crate::kinds::KindEdit>,
    levels: std::collections::BTreeMap<String, f32>,
    others: std::collections::BTreeMap<String, crate::project::OtherArea>,
    are: Option<fmt_odyssey::gff::Gff>,
    conversations: std::collections::BTreeMap<String, fmt_odyssey::gff::Gff>,
    selected: Option<Target>,
    also: (Option<Target>, Vec<Target>),
}

/// A project being edited.
pub struct Editor {
    pub project: Project,
    /// What is selected: the one whose details show and that the gizmo
    /// stands on when it is alone.
    pub selected: Option<Target>,
    /// What else is selected with it (`selection`), kept with the
    /// selection it belongs to: once `selected` is set to something else,
    /// it no longer counts.
    also: (Option<Target>, Vec<Target>),
    /// What the last paste added, to be selected with it.
    pasted: Vec<Target>,
    /// The lights the game places itself, as the view last drew them, each
    /// with where it comes from (`Target::Fitted`). Not part of the
    /// project.
    pub seen_lights: Vec<(crate::lights::PlacedLight, String)>,
    /// Whether each of `seen_lights` is from a fitting on a placeable.
    pub seen_on_placeable: Vec<bool>,
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
            also: (None, Vec::new()),
            pasted: Vec::new(),
            seen_lights: Vec::new(),
            seen_on_placeable: Vec::new(),
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
                .map(|room| (room.position, room.yaw)),
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
            Target::Lamp(at) => self
                .project
                .kjk
                .lamps
                .get(at)
                .map(|lamp| (lamp.at, lamp.yaw)),
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
            kinds: self.project.kinds.clone(),
            levels: self.project.levels.clone(),
            others: self.project.others.clone(),
            are: self.project.are.clone(),
            conversations: self.project.conversations.clone(),
            selected: self.selected,
            also: self.also.clone(),
        }
    }

    /// Everything selected, the selected one first: it and what else is
    /// selected with it.
    pub fn selection(&self) -> Vec<Target> {
        let mut all: Vec<Target> = self.selected.into_iter().collect();
        if self.also.0 == self.selected {
            for target in &self.also.1 {
                if !all.contains(target) {
                    all.push(*target);
                }
            }
        }
        all
    }

    /// Selects `target` alone, or, `adding`, toggles it in the selection:
    /// one not selected joins it (and is the one whose details show), one
    /// selected leaves it.
    pub fn select(&mut self, target: Option<Target>, adding: bool) {
        if !adding {
            self.selected = target;
            self.also = (None, Vec::new());
            return;
        }
        let Some(target) = target else {
            return;
        };
        let mut all = self.selection();
        if let Some(at) = all.iter().position(|had| *had == target) {
            all.remove(at);
        } else {
            all.insert(0, target);
        }
        self.select_all(all);
    }

    /// Selects `all`, the first the one whose details show.
    pub fn select_all(&mut self, all: Vec<Target>) {
        let mut all = all.into_iter();
        self.selected = all.next();
        self.also = (self.selected, all.collect());
    }

    /// The middle of the selection's places (metres), if anything is
    /// selected.
    pub fn selection_centre(&self) -> Option<[f32; 3]> {
        let places: Vec<[f32; 3]> = (self.selection().into_iter())
            .filter_map(|target| self.placement(target))
            .map(|(at, _)| at)
            .collect();
        if places.is_empty() {
            return None;
        }
        let count = places.len() as f32;
        Some(std::array::from_fn(|axis| {
            places.iter().map(|at| at[axis]).sum::<f32>() / count
        }))
    }

    /// The edits that move everything selected that can be changed by
    /// `by` (metres).
    pub fn moves_by(&self, by: [f32; 3]) -> Vec<Edit> {
        (self.selection().into_iter())
            .filter(|target| Self::editable(*target).is_ok())
            .filter_map(|target| {
                let (at, _) = self.placement(target)?;
                Some(Edit::Move(
                    target,
                    std::array::from_fn(|axis| at[axis] + by[axis]),
                ))
            })
            .collect()
    }

    /// The edits that turn everything selected `degrees` round `centre`
    /// (metres, about Z): each moves round it, and each that turns (not a
    /// trigger's or an encounter's outline) turns as much. `sin_cos` is
    /// the turn's sine and cosine.
    pub fn turns_round(&self, centre: [f32; 3], degrees: f32, (sin, cos): (f32, f32)) -> Vec<Edit> {
        let mut edits = Vec::new();
        for target in self.selection() {
            if Self::editable(target).is_err() {
                continue;
            }
            let Some((at, facing)) = self.placement(target) else {
                continue;
            };
            let (x, y) = (at[0] - centre[0], at[1] - centre[1]);
            let moved = [
                centre[0] + cos * x - sin * y,
                centre[1] + sin * x + cos * y,
                at[2],
            ];
            edits.push(Edit::Move(target, moved));
            if !matches!(
                target,
                Target::Object(Kind::Trigger | Kind::Encounter, _) | Target::Entry
            ) {
                edits.push(Edit::Turn(target, facing + degrees));
            }
        }
        edits
    }

    /// The edits that delete everything selected that can be deleted, each
    /// list from its end, so the places of those still to go hold.
    pub fn deletes(&self) -> Vec<Edit> {
        let index = |target: &Target| match target {
            Target::Object(_, at)
            | Target::Prop(at)
            | Target::Box(at)
            | Target::Room(at)
            | Target::Light(at)
            | Target::Fitted(at)
            | Target::Lamp(at) => *at,
            Target::Entry => 0,
        };
        let mut all: Vec<Target> = (self.selection().into_iter())
            .filter(|target| Self::editable(*target).is_ok() && *target != Target::Entry)
            .collect();
        all.sort_by_key(|target| std::cmp::Reverse(index(target)));
        all.into_iter().map(Edit::Delete).collect()
    }

    /// Makes `edits` one after another as one edit, kept for undo as one
    /// step: all of them, or, when one is refused, none. What each edit
    /// selects is selected afterwards, the first one's details shown; with
    /// none, what was selected stays.
    pub fn apply_many(&mut self, edits: Vec<Edit>) -> Result<(), String> {
        let before = self.state();
        match self.change_many(edits) {
            Ok(()) => {
                self.undo.push(before);
                self.redo.clear();
                self.changes += 1;
                Ok(())
            }
            Err(why) => {
                self.restore_quietly(before);
                Err(why)
            }
        }
    }

    /// `apply_many`, joining the last edit's undo step (a drag).
    pub fn apply_many_continuing(&mut self, edits: Vec<Edit>) -> Result<(), String> {
        if self.undo.is_empty() {
            return self.apply_many(edits);
        }
        let before = self.state();
        match self.change_many(edits) {
            Ok(()) => {
                self.redo.clear();
                self.changes += 1;
                Ok(())
            }
            Err(why) => {
                self.restore_quietly(before);
                Err(why)
            }
        }
    }

    fn change_many(&mut self, edits: Vec<Edit>) -> Result<(), String> {
        if edits.is_empty() {
            return Err("nothing selected can be changed so".into());
        }
        let was = self.selection();
        let mut chosen = Vec::new();
        for edit in edits {
            if let Some(target) = self.change(edit)?
                && !chosen.contains(&target)
            {
                chosen.push(target);
            }
        }
        if chosen.is_empty() {
            // Deleted, all of it: nothing stays selected.
            self.select_all(Vec::new());
        } else if chosen.iter().all(|target| was.contains(target)) {
            // Moved or turned: the selection stays as it was, its order too.
            self.select_all(was);
        } else {
            self.select_all(chosen);
        }
        Ok(())
    }

    fn restore(&mut self, state: State) {
        self.project.git = state.git;
        self.project.kjk = state.kjk;
        self.project.templates = state.templates;
        self.project.map = state.map;
        self.project.kinds = state.kinds;
        self.project.levels = state.levels;
        self.project.others = state.others;
        self.project.are = state.are;
        self.project.conversations = state.conversations;
        self.selected = state.selected;
        self.also = state.also;
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
                // A paste selects everything it added.
                let pasted = std::mem::take(&mut self.pasted);
                if pasted.len() > 1 {
                    self.select_all(pasted);
                }
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
        self.project.kinds = state.kinds;
        self.project.levels = state.levels;
        self.project.others = state.others;
        self.project.are = state.are;
        self.project.conversations = state.conversations;
        self.selected = state.selected;
        self.also = state.also;
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
            // A trigger's link is its entry's too (`module::PLACED_FIELDS`).
            template::Change::Link(_) => {
                if !matches!(kind, Kind::Door | Kind::Trigger) {
                    return Err("only a door or a trigger leads somewhere".into());
                }
                true
            }
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
            Target::Object(kind, _) if !EDITABLE.contains(&kind) => {
                Err(format!("a {kind:?} cannot be changed in the editor"))
            }
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
                    Target::Lamp(at) => kjk.lamps.get_mut(at).map(|l| l.at = to).is_some(),
                    Target::Fitted(_) => false,
                };
                moved.then_some(Some(target)).ok_or_else(|| missing(target))
            }
            Edit::Turn(target, degrees) => {
                Self::editable(target)?;
                if let Target::Object(Kind::Trigger | Kind::Encounter, _) = target {
                    return Err("a trigger's or an encounter's outline does not turn".into());
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
                    Target::Lamp(at) => kjk.lamps.get_mut(at).map(|l| l.yaw = degrees).is_some(),
                    // The room's copy is turned (`map::room_copy`).
                    Target::Room(at) => map
                        .as_mut()
                        .and_then(|map| map.rooms.get_mut(at))
                        .map(|room| room.yaw = degrees)
                        .is_some(),
                    Target::Fitted(_) => false,
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
                    Target::Lamp(at) => {
                        let copy = kjk.lamps.get(at).ok_or_else(|| missing(target))?.clone();
                        kjk.lamps.push(copy);
                        Ok(Some(Target::Lamp(kjk.lamps.len() - 1)))
                    }
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
                    Target::Lamp(at) => (at < kjk.lamps.len())
                        .then(|| kjk.lamps.remove(at))
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
                    return Err(format!("a {kind:?} cannot be added"));
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
            Edit::Paste { clip, at, taken } => {
                let added = crate::clip::paste(&mut self.project, &clip, at, &|name| {
                    taken.iter().any(|had| had == name)
                })?;
                self.pasted = added.clone();
                Ok(added.first().copied())
            }
            Edit::AddLink(link) => {
                if !link
                    .from
                    .iter()
                    .chain(&link.to)
                    .all(|value| value.is_finite())
                {
                    return Err("a link's ends must be finite".into());
                }
                kjk.links.push(link);
                Ok(self.selected)
            }
            Edit::RemoveLink(at) => {
                if at >= kjk.links.len() {
                    return Err(format!("there is no link {at}"));
                }
                kjk.links.remove(at);
                Ok(self.selected)
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
            Edit::SetLevel(room, level) => {
                match level {
                    Some(level) if level.is_finite() && (0.0..=4.0).contains(&level) => {
                        self.project.levels.insert(room.to_ascii_lowercase(), level);
                    }
                    Some(_) => return Err("a room's level is from 0 to 4".into()),
                    None => {
                        self.project.levels.remove(&room.to_ascii_lowercase());
                    }
                }
                Ok(self.selected)
            }
            Edit::SetKind(kind) => {
                let numbers = kind
                    .colour
                    .iter()
                    .chain([&kind.reach, &kind.strength, &kind.cone]);
                if !numbers.copied().all(f32::is_finite) || kind.reach <= 0.0 {
                    return Err(
                        "a kind's numbers must be finite, and it must reach some way".into(),
                    );
                }
                let kinds = &mut self.project.kinds;
                match kinds
                    .iter_mut()
                    .find(|had| had.texture == kind.texture && had.placeable == kind.placeable)
                {
                    Some(had) => *had = kind,
                    None => kinds.push(kind),
                }
                Ok(self.selected)
            }
            Edit::AddLight(light) => {
                light_problem(&light)?;
                kjk.lights.push(light);
                Ok(Some(Target::Light(kjk.lights.len() - 1)))
            }
            Edit::LinkOther {
                module,
                area,
                base,
                kind,
                index,
                link,
            } => {
                let area = area.to_ascii_lowercase();
                if area == self.project.area {
                    return Err(format!(
                        "{area} is the project's own area: select the object and set where it leads"
                    ));
                }
                if !matches!(kind, Kind::Door | Kind::Trigger) {
                    return Err("only a door or a trigger leads somewhere".into());
                }
                let other = match self.project.others.entry(area.clone()) {
                    std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
                    std::collections::btree_map::Entry::Vacant(free) => {
                        let git = base.ok_or(format!("{area}'s GIT was not read"))?;
                        free.insert(crate::project::OtherArea {
                            module: module.to_ascii_lowercase(),
                            git,
                        })
                    }
                };
                let mut item = git::item(&other.git, kind, index)
                    .ok_or(format!("{area} has no {kind:?} {index}"))?
                    .clone();
                crate::template::set_link(&mut item, &link)?;
                for field in item.fields {
                    let label = String::from_utf8_lossy(&field.label).into_owned();
                    git::set_entry_field(&mut other.git, kind, index, &label, field.value);
                }
                Ok(self.selected)
            }
            Edit::Outline(target, points) => {
                let Target::Object(kind @ (Kind::Trigger | Kind::Encounter), index) = target else {
                    return Err("only a trigger or an encounter has an outline".into());
                };
                if points.len() < 3 {
                    return Err("an outline has three points or more".into());
                }
                let at = self
                    .entry(kind, index)
                    .ok_or_else(|| missing(target))?
                    .position;
                let relative: Vec<[f32; 3]> = points
                    .iter()
                    .map(|point| [point[0] - at[0], point[1] - at[1], point[2] - at[2]])
                    .collect();
                git::set_outline(&mut self.project.git, kind, index, &relative);
                Ok(Some(target))
            }
            Edit::SpawnPoints(target, points) => {
                let Target::Object(Kind::Encounter, index) = target else {
                    return Err("only an encounter has spawn points".into());
                };
                if !git::set_spawn_points(&mut self.project.git, index, &points) {
                    return Err(missing(target));
                }
                Ok(Some(target))
            }
            Edit::Area { change, base } => {
                use crate::settings::{AreaChange, set_are, set_property};
                match change {
                    AreaChange::Property(label, value) => {
                        set_property(&mut self.project.git, &label, value);
                    }
                    AreaChange::Are(label, value) => {
                        let are = match self.project.map.as_mut() {
                            Some(map) => &mut map.are,
                            None => match self.project.are {
                                Some(ref mut own) => own,
                                None => self
                                    .project
                                    .are
                                    .insert(base.ok_or("the area's ARE was not read")?),
                            },
                        };
                        set_are(are, &label, value)?;
                    }
                    AreaChange::ModuleScript(label, script) => {
                        let map = self.project.map.as_mut().ok_or(
                            "an installed module's scripts are its own: every module.ifo has one name",
                        )?;
                        if !crate::settings::MODULE_SCRIPTS.contains(&label.as_str()) {
                            return Err(format!("{label} is not a module script"));
                        }
                        if script.len() > git::RESREF_LEN {
                            return Err(format!("{script:?} is longer than a script's name"));
                        }
                        map.module_scripts
                            .insert(label, script.to_ascii_lowercase());
                    }
                }
                Ok(self.selected)
            }
            Edit::NewConversation { name, base } => {
                let name = name.to_ascii_lowercase();
                if name.is_empty()
                    || name.len() > git::RESREF_LEN
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err(format!(
                        "{name:?}: a conversation's name is 1 to 16 letters, digits and _"
                    ));
                }
                if self.project.conversations.contains_key(&name) {
                    return Err(format!("the project has a conversation {name} already"));
                }
                self.project
                    .conversations
                    .insert(name, base.unwrap_or_else(crate::dialog::empty));
                Ok(self.selected)
            }
            Edit::Conversation(name, change) => {
                let conversation = self
                    .project
                    .conversations
                    .get_mut(&name)
                    .ok_or(format!("the project has no conversation {name}"))?;
                crate::dialog::apply(conversation, &change)?;
                Ok(self.selected)
            }
            Edit::DropConversation(name) => {
                self.project
                    .conversations
                    .remove(&name)
                    .ok_or(format!("the project has no conversation {name}"))?;
                Ok(self.selected)
            }
            Edit::DropOther(area) => {
                self.project
                    .others
                    .remove(&area.to_ascii_lowercase())
                    .ok_or(format!("the project does not change {area}"))?;
                Ok(self.selected)
            }
            Edit::AddLamp(lamp) => {
                crate::lamps::problem(&lamp)?;
                kjk.lamps.push(lamp);
                Ok(Some(Target::Lamp(kjk.lamps.len() - 1)))
            }
            Edit::SetLamp(at, lamp) => {
                crate::lamps::problem(&lamp)?;
                let slot = (kjk.lamps)
                    .get_mut(at)
                    .ok_or_else(|| missing(Target::Lamp(at)))?;
                *slot = lamp;
                Ok(Some(Target::Lamp(at)))
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
                    .chain(std::iter::once(&room.yaw))
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
