//! Objects copied out of a project and pasted into another, or into the
//! same one (`docs/LEVEL-EDIT.md`, "Many objects at once").
//!
//! A copy (`Clip::of`) holds what is selected: each object's GIT entry as
//! the project has it, the templates of the project's own that they use,
//! and the Jedi Academy props, collision boxes, lamps and lights placed by
//! hand. It is written as text (`Clip::write`), so it goes through the
//! system's clipboard: a first line that says what it is, the game, the
//! middle of what was copied, the entries and templates as KOTOR's own
//! bytes in hexadecimal, then the kjk-only things as `kjk-area.toml`
//! writes them.
//!
//! A paste (`Edit::Paste`) puts all of it where the copy's middle lands,
//! as one edit. A template the project has of that name already and holds
//! the same is used as it is. One of that name holding something else, or
//! one the level reads another of (`taken`), is pasted under a new name,
//! and its objects with it. The editor puts the game's own templates the
//! objects use into the copy too, and leaves out of a paste each one the
//! level reads the same already. A copy of one game is
//! not pasted into a project of the other.

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct};

use crate::area::KjkArea;
use crate::editor::Target;
use crate::git::{self, Kind};
use crate::project::Project;
use crate::template::Template;

/// The first line of a copy.
pub const HEAD: &str = "kjk level editor copy 1";
/// The line between a copy's own fields and its kjk-only things.
const AREA_MARK: &str = "# kjk-area";

/// What was copied (above).
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub game: String,
    /// The middle of what was copied, metres.
    pub centre: [f32; 3],
    /// The objects' GIT entries, each by its kind.
    pub entries: Vec<(Kind, GffStruct)>,
    /// The project's own templates they use, by name.
    pub templates: Vec<(String, Template)>,
    /// The props, boxes, lamps and lights (and nothing else of it).
    pub kjk: KjkArea,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("a copy's bytes are not hexadecimal".into());
    }
    (0..text.len() / 2)
        .map(|at| {
            u8::from_str_radix(&text[at * 2..at * 2 + 2], 16)
                .map_err(|_| "a copy's bytes are not hexadecimal".to_string())
        })
        .collect()
}

impl Clip {
    /// A copy of `targets` of `project`, round `centre`. Targets that are
    /// not things placed in the level (the entry, a room, a light the game
    /// places) are left out.
    pub fn of(project: &Project, targets: &[Target], centre: [f32; 3]) -> Clip {
        let mut clip = Clip {
            game: project.game.clone(),
            centre,
            entries: Vec::new(),
            templates: Vec::new(),
            kjk: KjkArea::default(),
        };
        for target in targets {
            match *target {
                Target::Object(kind, index) => {
                    let Some(item) = git::item(&project.git, kind, index) else {
                        continue;
                    };
                    let entry = git::entries(&project.git)
                        .into_iter()
                        .find(|entry| entry.kind == kind && entry.index == index);
                    if let Some(made) = entry.and_then(|entry| {
                        project
                            .templates
                            .get(&entry.template)
                            .map(|made| (entry.template, made))
                    }) && !clip.templates.iter().any(|(name, _)| *name == made.0)
                    {
                        clip.templates.push((made.0, made.1.clone()));
                    }
                    clip.entries.push((kind, item.clone()));
                }
                Target::Prop(at) => clip.kjk.props.extend(project.kjk.props.get(at).cloned()),
                Target::Box(at) => clip.kjk.boxes.extend(project.kjk.boxes.get(at).cloned()),
                Target::Lamp(at) => clip.kjk.lamps.extend(project.kjk.lamps.get(at).cloned()),
                Target::Light(at) => clip.kjk.lights.extend(project.kjk.lights.get(at).cloned()),
                Target::Room(_) | Target::Entry | Target::Fitted(_) => {}
            }
        }
        clip
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
            && self.kjk.props.is_empty()
            && self.kjk.boxes.is_empty()
            && self.kjk.lamps.is_empty()
            && self.kjk.lights.is_empty()
    }

    /// The copy as text (above).
    pub fn write(&self) -> String {
        let mut git = git::empty();
        let mut kinds = Vec::new();
        for (kind, item) in &self.entries {
            git::push(&mut git, *kind, item.clone());
            kinds.push(format!("{:?}", kind_word(*kind)));
        }
        let mut out = format!(
            "{HEAD}\ngame = {:?}\ncentre = [{:?}, {:?}, {:?}]\nkinds = [{}]\ngit = \"{}\"\n",
            self.game,
            self.centre[0],
            self.centre[1],
            self.centre[2],
            kinds.join(", "),
            hex(&git.write()),
        );
        for (name, made) in &self.templates {
            out.push_str(&format!(
                "\n[[template]]\nname = {name:?}\nkind = {:?}\nfrom = {:?}\ngff = \"{}\"\n",
                crate::template::extension(made.restype),
                made.from,
                hex(&made.gff.write()),
            ));
        }
        out.push_str(&format!("\n{AREA_MARK}\n{}", self.kjk.write()));
        out
    }

    /// The copy `text` holds (`write`).
    pub fn read(text: &str) -> Result<Clip, String> {
        let text = text.replace("\r\n", "\n");
        let rest = text
            .strip_prefix(HEAD)
            .ok_or("this is not a copy of the level editor's objects")?;
        let (fields, area) = rest.split_once(AREA_MARK).unwrap_or((rest, ""));
        let document: toml_edit::DocumentMut = fields
            .parse()
            .map_err(|error| format!("the copy does not read: {error}"))?;
        let text_of = |key: &str| {
            document
                .get(key)
                .and_then(toml_edit::Item::as_str)
                .map(str::to_string)
                .ok_or(format!("the copy has no {key}"))
        };
        let number = |item: &toml_edit::Value| {
            item.as_float()
                .or_else(|| item.as_integer().map(|whole| whole as f64))
                .map(|value| value as f32)
        };
        let centre = document
            .get("centre")
            .and_then(toml_edit::Item::as_array)
            .and_then(|values| {
                let values: Vec<f32> = values.iter().filter_map(number).collect();
                (values.len() == 3).then(|| [values[0], values[1], values[2]])
            })
            .ok_or("the copy has no centre")?;
        let kinds: Vec<Kind> = document
            .get("kinds")
            .and_then(toml_edit::Item::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| kind_of(value.as_str()?))
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        let git = Gff::parse(&unhex(&text_of("git")?)?)
            .map_err(|error| format!("the copy's objects do not read: {error}"))?;
        // The entries in the order they were copied: each kind's list in
        // turn, as `kinds` names them.
        let mut taken: std::collections::BTreeMap<Kind, usize> = std::collections::BTreeMap::new();
        let mut entries = Vec::new();
        for kind in kinds {
            let at = taken.entry(kind).or_insert(0);
            let item = git::item(&git, kind, *at)
                .ok_or("the copy names more objects than it holds")?
                .clone();
            *at += 1;
            entries.push((kind, item));
        }
        let mut templates = Vec::new();
        if let Some(tables) = document
            .get("template")
            .and_then(toml_edit::Item::as_array_of_tables)
        {
            for table in tables {
                let field = |key: &str| {
                    table
                        .get(key)
                        .and_then(toml_edit::Item::as_str)
                        .map(str::to_string)
                        .ok_or(format!("a copied template has no {key}"))
                };
                let extension = field("kind")?;
                let restype = crate::template::TYPES
                    .iter()
                    .find(|(_, of)| *of == extension)
                    .map(|(restype, _)| *restype)
                    .ok_or(format!("{extension} is not a template's kind"))?;
                let gff = Gff::parse(&unhex(&field("gff")?)?)
                    .map_err(|error| format!("a copied template does not read: {error}"))?;
                templates.push((
                    field("name")?.to_ascii_lowercase(),
                    Template {
                        restype,
                        from: field("from")?,
                        gff,
                    },
                ));
            }
        }
        Ok(Clip {
            game: text_of("game")?,
            centre,
            entries,
            templates,
            kjk: KjkArea::parse(area)?,
        })
    }
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Creature => "creature",
        Kind::Placeable => "placeable",
        Kind::Door => "door",
        Kind::Waypoint => "waypoint",
        Kind::Trigger => "trigger",
        Kind::Sound => "sound",
        Kind::Encounter => "encounter",
        Kind::Store => "store",
    }
}

fn kind_of(word: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|kind| kind_word(*kind) == word)
}

/// The template field a GIT entry of `kind` names its template by.
fn template_label(kind: Kind) -> &'static str {
    if kind == Kind::Store {
        "ResRef"
    } else {
        "TemplateResRef"
    }
}

/// Puts `clip` into `project` with its middle at `at`: the targets added,
/// in the order the copy holds them. `taken` says whether the install has
/// a template of a name, which a pasted template must not take.
pub fn paste(
    project: &mut Project,
    clip: &Clip,
    at: [f32; 3],
    taken: &dyn Fn(&str) -> bool,
) -> Result<Vec<Target>, String> {
    if !clip.game.eq_ignore_ascii_case(&project.game) {
        return Err(format!(
            "the copy is of {}'s objects, and this project is of {}",
            clip.game, project.game
        ));
    }
    if clip.is_empty() {
        return Err("the copy holds nothing".into());
    }
    let by: [f32; 3] = std::array::from_fn(|axis| at[axis] - clip.centre[axis]);
    let moved =
        |point: [f32; 3]| -> [f32; 3] { std::array::from_fn(|axis| point[axis] + by[axis]) };
    // The templates: the project's own of the same name and bytes is used,
    // one of the same name but other bytes is pasted under a new name.
    let mut renamed: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (name, made) in &clip.templates {
        match project.templates.get(name) {
            Some(had) if had.gff == made.gff => {}
            // The level reads another template of this name (its own
            // module's, or the project's): the copy takes a new name, so
            // the level's own objects keep theirs.
            _ if taken(name) || project.templates.contains_key(name) => {
                let templates = &project.templates;
                let fresh = crate::template::fresh_name(&made.from, |candidate| {
                    templates.contains_key(candidate)
                        || taken(candidate)
                        || clip.templates.iter().any(|(other, _)| other == candidate)
                });
                project.templates.insert(fresh.clone(), made.clone());
                renamed.insert(name.clone(), fresh);
            }
            _ => {
                project.templates.insert(name.clone(), made.clone());
            }
        }
    }
    let mut added = Vec::new();
    for (kind, item) in &clip.entries {
        let mut item = item.clone();
        let label = template_label(*kind);
        if let Some(name) = item.resref(label).map(|name| name.to_ascii_lowercase())
            && let Some(fresh) = renamed.get(&name)
        {
            git::set_field(
                &mut item,
                label,
                fmt_odyssey::gff::GffValue::ResRef(fresh.clone().into_bytes()),
            );
        }
        let index = git::push(&mut project.git, *kind, item);
        let entry = git::entries(&project.git)
            .into_iter()
            .find(|entry| entry.kind == *kind && entry.index == index)
            .ok_or("a pasted object does not read")?;
        git::set_position(&mut project.git, *kind, index, moved(entry.position));
        added.push(Target::Object(*kind, index));
    }
    for prop in &clip.kjk.props {
        let mut prop = prop.clone();
        prop.position = moved(prop.position);
        if project.kjk.props.iter().any(|had| had.name == prop.name)
            || project.kjk.boxes.iter().any(|had| had.name == prop.name)
        {
            prop.name = project.kjk.free_name("prop");
        }
        project.kjk.props.push(prop);
        added.push(Target::Prop(project.kjk.props.len() - 1));
    }
    for shape in &clip.kjk.boxes {
        let mut shape = shape.clone();
        shape.centre = moved(shape.centre);
        if project.kjk.boxes.iter().any(|had| had.name == shape.name)
            || project.kjk.props.iter().any(|had| had.name == shape.name)
        {
            shape.name = project.kjk.free_name("box");
        }
        project.kjk.boxes.push(shape);
        added.push(Target::Box(project.kjk.boxes.len() - 1));
    }
    for lamp in &clip.kjk.lamps {
        let mut lamp = lamp.clone();
        lamp.at = moved(lamp.at);
        project.kjk.lamps.push(lamp);
        added.push(Target::Lamp(project.kjk.lamps.len() - 1));
    }
    for light in &clip.kjk.lights {
        let mut light = light.clone();
        light.at = moved(light.at);
        project.kjk.lights.push(light);
        added.push(Target::Light(project.kjk.lights.len() - 1));
    }
    Ok(added)
}

/// The restype of a template a GIT entry of `kind` names, for a check that
/// the install has it.
pub fn template_type(kind: Kind) -> Option<ResType> {
    crate::template::restype(kind)
}
