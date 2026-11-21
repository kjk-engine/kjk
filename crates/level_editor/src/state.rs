//! The editor's state: the project being edited, the scene drawn from it,
//! the collision built from it, and the `edit` commands that change it.
//! The panels, the gizmo and the asset picker (`ui`) turn clicks and keys
//! into these commands and edits.
//!
//! The scene is drawn again from the project after edits (`redraw`): the
//! area's resolver with the edited GIT over it, the module's scene made
//! from it as `kjk module` makes it, then the Jedi Academy props, the
//! collision boxes and the markers added. While an object is being moved,
//! its marker and its model move at once (`follow`), and the redraw waits
//! until the edits stop for a moment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::module::ModuleIr;
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use bevy::prelude::*;
use console::DumpFacts;
use fmt_odyssey::ResType;
use level_edit::editor::{ADDABLE, EDITABLE};
use level_edit::{BoxRole, Change, CollisionBox, Edit, Editor, Entry, Kind, Prop, Target};
use vfs::Game;
use vfs::resolver::{Layer, Location, Rank, Resolver};

use kjk_game::project_props::{self, PropModel};
use kjk_game::scene::{FlyCamera, SceneContent, SceneModel, game_to_bevy};
use kjk_game::viewer::{LoadedScene, Placed, ResolverSource};

/// How many frames without an edit before the scene is drawn again.
const SETTLE_FRAMES: u32 = 12;
/// How far one press of an arrow key moves the selection, metres.
pub(crate) const STEP_M: f32 = 0.25;
/// How far one press of `[` or `]` turns it, degrees.
pub(crate) const TURN_DEG: f32 = 15.0;
/// How far a marker sits above its object's origin, metres.
pub(crate) const MARKER_RISE: f32 = 0.35;
/// The size a new collision box starts at, metres: a ledge.
const NEW_BOX: [f32; 3] = [2.0, 1.0, 0.3];
/// How far ahead of the camera something is placed when nothing is under
/// the screen's centre, metres.
const PLACE_AHEAD_M: f32 = 4.0;

/// Everything the editor starts from, made before the window opens
/// (`main`).
pub struct EditSetup {
    /// The project as its folder holds it, or as it was just made.
    pub project: level_edit::Project,
    pub game: Game,
    /// The install's resolver with the module's archives and the project's
    /// folder (`vfs::project`) in it.
    pub resolver: Resolver,
    /// Jedi Academy's archives, for props; `None` without the game.
    pub ja: Option<vfs::ja::JaStack>,
    /// The install's placeable and creature templates, and Jedi Academy's
    /// map objects.
    pub placeables: Vec<String>,
    pub creatures: Vec<String>,
    pub models: Vec<String>,
    /// The install's compiled scripts and item templates, for the scripts
    /// and items an object's template names.
    pub scripts: Vec<String>,
    /// The game's routines, from its `nwscript.nss`, for the script viewer.
    pub catalog: Option<script_ncs::Catalog>,
    /// What `nwscript.nss` says of each routine (`catalog::notes`).
    pub notes: BTreeMap<String, String>,
    /// The install's talk table (`dialog.tlk`), for the game's lines of a
    /// conversation.
    pub talk: Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>>,
    pub items: Vec<String>,
    /// The install's door templates (`.utd`).
    pub doors: Vec<String>,
    /// The install's modules, which a door or a trigger can lead to.
    pub modules: Vec<String>,
    /// The globals scripts may keep (`globalcat.2da`): each one's name and
    /// type (`number`, `boolean`, `string`, `location`).
    pub globals: Vec<(String, String)>,
    /// The install's waypoint, trigger, sound and store templates.
    pub waypoints: Vec<String>,
    pub triggers: Vec<String>,
    pub sounds: Vec<String>,
    pub stores: Vec<String>,
    /// The install's encounter templates (`.ute`).
    pub encounters: Vec<String>,
    /// The install's rooms, for a new map (`LibraryRoom`).
    pub rooms: Vec<LibraryRoom>,
    /// The folder of the fitted lights' rule data (`data/lights`) that an
    /// installed level's lights placed by hand are saved to: the one
    /// `KJK_LIGHT_DATA` names, else the kjk checkout's this program was
    /// built in. `None` when neither is found.
    pub light_data: Option<PathBuf>,
    /// The bakes of the fitted lights found for this area (`kjk
    /// detail-bake`): each folder of baked lightmaps, for the Baked view.
    pub bakes: Vec<PathBuf>,
    /// The `kjk` that `edit play` and `edit audit` start: the one beside
    /// this program.
    pub program: PathBuf,
    /// Whether that `kjk` has the verbs that check and measure (its
    /// `probes` feature, a build from source): the containment audit and
    /// the bake. A player's build, as a release package holds, has not.
    pub tools: bool,
}

/// Why an installed level's lights and lamps placed by hand are not saved
/// by a build that has no light data folder.
pub(crate) const NO_LIGHT_DATA: &str = "this editor cannot save lights and lamps placed by hand in \
     an installed level: they are kept in kjk's light data, which comes with kjk built from \
     source (or set KJK_LIGHT_DATA to a data/lights folder). In a new map they are saved \
     with the project";

/// Why the audit and the bake are not there with a player's build of kjk.
pub(crate) const NO_TOOLS: &str = "the kjk beside the editor is a player's build, without the \
     audit and the bake: they come with kjk built from source (docs/BUILD.md)";

/// A room of the install that a new map can place: its model, where its
/// own area's layout places it, and that area.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryRoom {
    pub model: String,
    pub from: [f32; 3],
    pub area: String,
}

/// What the editor found of the navigation mesh it drew.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NavFacts {
    pub(crate) faces: usize,
    pub(crate) islands: usize,
    pub(crate) portals: usize,
    /// Where routes are counted from: a new map's entry, else the largest
    /// island.
    pub(crate) from: &'static str,
    /// Faces of other islands than that one.
    pub(crate) apart: usize,
    /// Whether a new map's entry stands on no face of the mesh (routes are
    /// then counted from the largest island).
    pub(crate) entry_off: bool,
    /// Each link between floors (`KjkArea::links`), in order: whether the
    /// mesh takes it and a plain jump or a drop crosses it, else why not.
    pub(crate) links: Vec<Result<(), String>>,
    /// Faces of other islands that the links lead to from the one routes
    /// are counted from.
    pub(crate) by_links: usize,
}

/// A room's level as its stored lightmaps show it, about as `kjk
/// detail-bake` measures it: `original` is the mean of their texels' levels,
/// and `lit` the same mean with each texel counted by its own level. The
/// bake counts only the texels a surface covers; this counts every texel
/// of the room's lightmaps, so it is a guide.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RoomMeasure {
    pub(crate) room: String,
    pub(crate) original: f32,
    pub(crate) lit: f32,
}

/// Each room's level from the lightmaps of `scene` (`RoomMeasure`).
fn measure_rooms(scene: &LoadedScene) -> Vec<RoomMeasure> {
    use asset_odyssey::texture::Encoding;
    let luma = |texture: &asset_odyssey::texture::TextureIr| -> Option<Vec<f32>> {
        // Its first level no wider or taller than 128 texels.
        let size = |level: usize| {
            (
                (texture.width as usize >> level).max(1),
                (texture.height as usize >> level).max(1),
            )
        };
        let level = (0..texture.mips.len())
            .find(|&level| size(level).0.max(size(level).1) <= 128)
            .or(texture.mips.len().checked_sub(1))?;
        let (width, height) = size(level);
        let data = &texture.mips[level];
        let rgba = match texture.encoding {
            Encoding::Rgba8 => data.clone(),
            Encoding::Bc1 | Encoding::Bc1Holes => {
                fmt_odyssey::dxt::decode(data, width, height, false)?
            }
            Encoding::Bc3 => fmt_odyssey::dxt::decode(data, width, height, true)?,
        };
        Some(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .take(width * height)
                .map(|[r, g, b, _]| {
                    (0.2126 * f32::from(*r) + 0.7152 * f32::from(*g) + 0.0722 * f32::from(*b))
                        / 255.0
                })
                .collect(),
        )
    };
    let mut out = Vec::new();
    for room in &scene.rooms {
        let Some(model) = room.model.and_then(|model| scene.models.get(model)) else {
            continue;
        };
        let names: std::collections::BTreeSet<&String> = model
            .ir
            .meshes
            .iter()
            .filter_map(|mesh| mesh.lightmap.as_ref())
            .collect();
        let (mut sum, mut squares, mut count) = (0.0f64, 0.0f64, 0usize);
        for name in names {
            let Some(levels) = scene.textures.get(name.as_str()).and_then(luma) else {
                continue;
            };
            for level in levels {
                sum += f64::from(level);
                squares += f64::from(level * level);
                count += 1;
            }
        }
        if count > 0 {
            out.push(RoomMeasure {
                room: room.name.to_ascii_lowercase(),
                original: (sum / count as f64) as f32,
                lit: (squares / sum.max(1e-9)) as f32,
            });
        }
    }
    out
}

/// The rooms' copies by their name, their model, and how far they moved
/// and turned (each as bits).
type MadeRooms = BTreeMap<(String, String, [u32; 4]), Vec<(String, ResType, Vec<u8>)>>;

/// What a scene's drawing placed for each thing the project holds.
#[derive(Default)]
pub(crate) struct Drawn {
    /// Each target's own model roots (not attached ones). Its marker is an
    /// icon over the view (`markers`), not a model of the scene.
    pub(crate) things: Vec<(Target, Vec<usize>)>,
    /// The collision overlay's model, when drawn.
    pub(crate) overlay: Option<usize>,
    /// Each new map room's yaw as its copy was drawn turned: a room turned
    /// since is shown turned by the difference (`follow`).
    pub(crate) room_yaws: Vec<f32>,
}

/// Where a running audit's output arrives, or why it did not start.
type AuditResult = std::sync::Arc<std::sync::Mutex<Option<Result<std::process::Output, String>>>>;

/// The editor's state.
#[derive(Resource)]
pub struct EditorState {
    pub(crate) setup: EditSetup,
    /// The edits are made here.
    pub editor: Editor,
    pub(crate) drawn: Drawn,
    built: u64,
    /// The editor's count of changes after the last edit that only moved
    /// or turned things the drawing follows (`follows`), when every change
    /// since the scene was drawn was such an edit. The scene is then not
    /// drawn again: `follow` has put each moved thing where it now is.
    followed: u64,
    /// Frames since the last edit.
    pub(crate) quiet: u32,
    pub(crate) prop_models: BTreeMap<String, Result<PropModel, String>>,
    /// The lamps the asset picker offers (`lamps`), found a few models a
    /// frame.
    pub(crate) lamp_library: crate::lamps::LampLibrary,
    /// Whether each creature template of the area has a conversation, and
    /// whether it updates the journal (`markers::creature_talks`), read when
    /// the scene is drawn.
    pub(crate) creature_talks: BTreeMap<String, Option<bool>>,
    /// The rooms' copies made so far (`level_edit::map::room_copy`).
    made_rooms: MadeRooms,
    collision: Option<(u64, collision_odyssey::ModuleCollision)>,
    pub(crate) show_collision: bool,
    shown_collision: bool,
    /// How the view lights the rooms (`fitted_lights::Mode`), the bake the
    /// Baked view shows, and whether the lights are drawn; with what the
    /// scene was last drawn with.
    pub(crate) light_view: kjk_game::fitted_lights::Mode,
    pub(crate) bake: Option<PathBuf>,
    pub(crate) show_lights: bool,
    /// Each kind of fitting the editor changed, as the light data held it
    /// before: what the view shows again when the change is undone.
    pub(crate) kind_originals: BTreeMap<(String, bool), level_edit::kinds::KindEdit>,
    /// The module's bake as `bake.toml` says it, the levels its file held
    /// when the editor opened (or last saved), each room's level from its
    /// stored lightmaps (measured once, when the Room levels window first
    /// opens), and whether that window is open.
    pub(crate) bake_entry: level_edit::levels::BakeEntry,
    pub(crate) levels_original: BTreeMap<String, f32>,
    pub(crate) room_measures: Option<Vec<RoomMeasure>>,
    pub(crate) show_levels: bool,
    /// Why each of the project's scripts did not compile at the last save,
    /// by name, and how many times they have been compiled, so the script
    /// viewer reads them again.
    pub(crate) script_errors: BTreeMap<String, String>,
    pub(crate) script_round: u32,
    /// The scripts written in the script viewer, by name, as they are being
    /// edited (`script_edit`), and the names of the scripts in the
    /// project's folder.
    pub(crate) drafts: BTreeMap<String, crate::script_edit::Draft>,
    pub(crate) project_scripts: std::collections::BTreeSet<String>,
    /// A script being tried (`script try`), and how the last try went.
    pub(crate) script_try: Option<crate::script_edit::Try>,
    pub(crate) tried: Option<crate::script_edit::Outcome>,
    /// Other modules as a link reads them: each one's entry area and that
    /// area's GIT as the install has it, read once.
    pub(crate) module_facts: BTreeMap<String, Result<(String, fmt_odyssey::gff::Gff), String>>,
    /// A bake running (`bake`), and the label of one asked for, which
    /// starts when the scene is next drawn (it bakes from the scene's
    /// lights).
    pub(crate) bake_job: Option<crate::bake::BakeJob>,
    bake_request: Option<String>,
    /// How the lights were drawn: the view's mode, its bake, and whether
    /// the lights are shown. The selected light's reach is drawn over the
    /// view (`ui::reach_in_view`), so a selection never draws the scene
    /// again.
    shown_lights: (kjk_game::fitted_lights::Mode, Option<PathBuf>, bool),
    /// Whether the navigation mesh is drawn, and what was found of it when
    /// it last was.
    pub(crate) show_nav: bool,
    shown_nav: bool,
    pub(crate) nav_facts: Option<NavFacts>,
    /// A gizmo is being dragged: the scene is not drawn again until it is
    /// let go.
    pub(crate) dragging: bool,
    /// A click `edit pick` asks for, at a point of the view (pixels).
    pub(crate) pending_pick: Option<Vec2>,
    /// Whether that pick joins the selection (Ctrl held), or leaves it.
    pub(crate) pending_pick_adds: bool,
    /// Objects copied (`edit copy`), for the window's clipboard.
    pub(crate) copied: Option<String>,
    /// A command a toolbar or details button asked for, run after the
    /// panels are laid out.
    pub(crate) pending_command: Option<String>,
    /// `edit ui ...` commands for the window's own state.
    pub(crate) pending_ui: Vec<Vec<String>>,
    pub(crate) messages: Vec<String>,
    pub(crate) audit: Vec<String>,
    /// A recovery copy an editor that did not close left of this project
    /// (`recover`), until it is taken or discarded.
    pub(crate) recovery: Option<PathBuf>,
    /// When this editor's own recovery copy was last written.
    pub(crate) autosave: crate::recover::Autosave,
    /// The kinds of object whose markers and labels the view hides.
    pub(crate) hidden: Vec<Kind>,
    /// Why the last edit was refused and when, for the view to show over
    /// the level for a few seconds.
    pub(crate) refusal: Option<(std::time::Instant, String)>,
    /// Whether the editor has said that this build cannot save an
    /// installed level's lights and lamps placed by hand.
    warned_lights: bool,
    /// A picture of the window the agent link asked for, and the file it
    /// goes to (`ui::take_shot`).
    pub(crate) pending_shot: Option<PathBuf>,
    /// Why the last edit was refused (`refuse`), until the command that
    /// asked for it takes it.
    refused: Option<String>,
    /// The install's templates read so far (`template_gff`), by name and
    /// type; `None` for one that does not read. The install does not change
    /// while the editor is open, so each is read and parsed once.
    install_templates: std::sync::Mutex<BTreeMap<(String, ResType), Option<fmt_odyssey::gff::Gff>>>,
    /// An audit running (`edit audit`): when it started, and where its
    /// output arrives.
    pub(crate) audit_job: Option<(std::time::Instant, AuditResult)>,
    redraws: u32,
}

impl EditorState {
    /// The state `setup` starts from, with its scene drawn.
    pub fn new(setup: EditSetup) -> Result<(EditorState, LoadedScene), String> {
        let mut project = setup.project.clone();
        // An installed level's lights placed by hand are the light data's
        // (`fittings.toml`), not the project's.
        if project.map.is_none()
            && let Some(folder) = &setup.light_data
        {
            let path = folder.join("fittings.toml");
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            project.kjk.lights =
                level_edit::lights::read_placed(&text, &project.game, &project.module)?;
            project.kjk.lamps =
                level_edit::lamps::read_lamps(&text, &project.game, &project.module)?;
        }
        // The module's bake, and the levels its rooms aim at.
        let (bake_entry, levels_original) = match &setup.light_data {
            Some(folder) => {
                let entry = std::fs::read_to_string(folder.join("bake.toml"))
                    .map(|text| level_edit::levels::bake_entry(&text, &project.module))
                    .unwrap_or_else(|_| level_edit::levels::BakeEntry::default());
                let levels = (entry.levels.as_ref())
                    .and_then(|file| std::fs::read_to_string(folder.join(file)).ok())
                    .map(|text| level_edit::levels::read_levels(&text))
                    .unwrap_or_else(BTreeMap::new);
                (entry, levels)
            }
            None => (level_edit::levels::BakeEntry::default(), BTreeMap::new()),
        };
        project.levels = levels_original.clone();
        let project_scripts = crate::script_edit::project_scripts(&project.folder);
        let recovery = crate::recover::found(&project.name);
        let editor = Editor::new(project);
        let mut state = EditorState {
            setup,
            editor,
            drawn: Drawn::default(),
            built: u64::MAX,
            followed: u64::MAX,
            quiet: SETTLE_FRAMES,
            prop_models: BTreeMap::new(),
            lamp_library: Default::default(),
            creature_talks: BTreeMap::new(),
            made_rooms: BTreeMap::new(),
            collision: None,
            show_collision: false,
            shown_collision: false,
            show_nav: false,
            shown_nav: false,
            light_view: kjk_game::fitted_lights::mode(),
            bake: None,
            show_lights: false,
            kind_originals: BTreeMap::new(),
            bake_entry,
            levels_original,
            room_measures: None,
            show_levels: false,
            script_errors: BTreeMap::new(),
            script_round: 0,
            drafts: BTreeMap::new(),
            project_scripts,
            script_try: None,
            tried: None,
            module_facts: BTreeMap::new(),
            bake_job: None,
            bake_request: None,
            shown_lights: (kjk_game::fitted_lights::Mode::Off, None, false),
            nav_facts: None,
            dragging: false,
            pending_pick: None,
            pending_pick_adds: false,
            copied: None,
            pending_command: None,
            pending_ui: Vec::new(),
            messages: Vec::new(),
            audit: Vec::new(),
            recovery,
            autosave: Default::default(),
            hidden: Vec::new(),
            refusal: None,
            warned_lights: false,
            pending_shot: None,
            refused: None,
            install_templates: Default::default(),
            audit_job: None,
            redraws: 0,
        };
        let scene = draw(&mut state)?;
        if let Some(folder) = &state.recovery {
            let what = crate::recover::describe(folder);
            state.say(format!(
                "work that was not saved was found, {what}: Recover takes it, Discard removes it"
            ));
        }
        Ok((state, scene))
    }
}

/// Turns the editor on: `setup`'s scene is drawn, and the keys, clicks and
/// `edit` commands edit it.
impl EditorState {
    /// Whether anything is not saved: the project's edits, or a script
    /// written in the viewer.
    pub(crate) fn unsaved(&self) -> bool {
        self.editor.unsaved() || self.drafts.values().any(crate::script_edit::Draft::changed)
    }

    /// Writes every script changed in the viewer to the project's folder.
    pub(crate) fn write_drafts(&mut self) -> Result<(), String> {
        let folder = self.editor.project.folder.clone();
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        let mut written = 0;
        for (name, draft) in &mut self.drafts {
            if draft.changed() {
                crate::script_edit::write(&folder, name, draft)?;
                written += 1;
            }
        }
        if written > 0 {
            self.project_scripts = crate::script_edit::project_scripts(&folder);
            let scripts = if written == 1 { "script" } else { "scripts" };
            self.say(format!("wrote {written} {scripts}"));
        }
        Ok(())
    }

    /// The script `name` held as a draft: the project's own, read from its
    /// folder, else a copy of the install's source, else a new one.
    pub(crate) fn draft_of(&mut self, name: &str) -> Result<(), String> {
        if self.drafts.contains_key(name) {
            return Ok(());
        }
        if let Some(problem) = crate::script_edit::name_problem(name) {
            return Err(problem);
        }
        let text_of = |bytes: Vec<u8>| bytes.into_iter().map(char::from).collect::<String>();
        let own = std::fs::read(self.editor.project.folder.join(format!("{name}.nss")))
            .ok()
            .map(text_of);
        let draft = match own {
            Some(text) => crate::script_edit::Draft {
                saved: text.clone(),
                text,
            },
            None => {
                // The install's source, else its compiled script read back
                // into source, else a new script.
                let decompiled = || {
                    let bytes = self.setup.resolver.read(name, ResType::NCS)?;
                    let ncs = fmt_odyssey::ncs::Ncs::parse(&bytes).ok()?;
                    let catalog = self.setup.catalog.as_ref()?;
                    crate::script_view::decompiled(name, &ncs, catalog).ok()
                };
                let text = self
                    .setup
                    .resolver
                    .read(name, ResType::NSS)
                    .map(text_of)
                    .or_else(decompiled)
                    .unwrap_or_else(|| crate::script_edit::template(name, false));
                // Not in the folder yet: it is written at the next save.
                crate::script_edit::Draft {
                    text,
                    saved: String::new(),
                }
            }
        };
        self.drafts.insert(name.to_string(), draft);
        Ok(())
    }

    pub(crate) fn say(&mut self, message: impl Into<String>) {
        let message = message.into();
        info!(target: "editor", "{message}");
        self.messages.push(message);
        let over = self.messages.len().saturating_sub(8);
        self.messages.drain(..over);
    }

    /// Says why an edit was refused, and keeps it for the command that
    /// asked for the edit (`command`), which answers with it.
    fn refuse(&mut self, why: String) {
        self.refused = Some(why.clone());
        self.refusal = Some((std::time::Instant::now(), why.clone()));
        self.say(why);
    }

    pub(crate) fn apply(&mut self, edit: Edit) {
        // An installed level's lights and lamps placed by hand are saved to
        // kjk's light data, which a packaged build does not have.
        if matches!(edit, Edit::AddLight(_) | Edit::AddLamp(_))
            && self.editor.project.map.is_none()
            && self.setup.light_data.is_none()
            && !self.warned_lights
        {
            self.warned_lights = true;
            self.say(NO_LIGHT_DATA);
        }
        let (before, follows) = (self.editor.changes(), self.follows(&edit));
        if let Err(why) = self.editor.apply(edit) {
            self.refuse(why);
        }
        self.followed_to(before, follows);
        self.quiet = 0;
    }

    /// `edits` made as one, one undo step (`Editor::apply_many`): what is
    /// done to everything selected.
    pub(crate) fn apply_many(&mut self, edits: Vec<Edit>) {
        let (before, follows) = (
            self.editor.changes(),
            !edits.is_empty() && edits.iter().all(|edit| self.follows(edit)),
        );
        if let Err(why) = self.editor.apply_many(edits) {
            self.refuse(why);
        }
        self.followed_to(before, follows);
        self.quiet = 0;
    }

    /// `apply_many`, joining the last edit's undo step (a drag).
    pub(crate) fn apply_many_continuing(&mut self, edits: Vec<Edit>) {
        let (before, follows) = (
            self.editor.changes(),
            !edits.is_empty() && edits.iter().all(|edit| self.follows(edit)),
        );
        if let Err(why) = self.editor.apply_many_continuing(edits) {
            self.refuse(why);
        }
        self.followed_to(before, follows);
        self.quiet = 0;
    }

    /// `apply`, joining the last edit's undo step (a drag).
    pub(crate) fn apply_continuing(&mut self, edit: Edit) {
        let (before, follows) = (self.editor.changes(), self.follows(&edit));
        if let Err(why) = self.editor.apply_continuing(edit) {
            self.refuse(why);
        }
        self.followed_to(before, follows);
        self.quiet = 0;
    }

    /// Whether `edit` only moves or turns a thing that `follow` puts where
    /// it now is, with nothing else drawn changing with it. A thing whose
    /// fittings light the room (a placeable, a door, a lamp, a light, a
    /// new map's room) counts only while the view shows the game's own
    /// lightmaps. The collision and the navigation mesh, while shown, are
    /// drawn again after any move.
    fn follows(&self, edit: &Edit) -> bool {
        use kjk_game::fitted_lights::Mode;
        let target = match edit {
            Edit::Move(target, _) | Edit::Turn(target, _) => *target,
            // A lamp moved or turned in its details, as the gizmo turns it.
            Edit::SetLamp(at, lamp) => {
                let Some(was) = self.editor.project.kjk.lamps.get(*at) else {
                    return false;
                };
                let placed_alone = level_edit::PlacedLamp {
                    at: was.at,
                    yaw: was.yaw,
                    pitch: was.pitch,
                    roll: was.roll,
                    ..lamp.clone()
                } == *was;
                if !placed_alone {
                    return false;
                }
                Target::Lamp(*at)
            }
            _ => return false,
        };
        if self.show_collision || self.show_nav {
            return false;
        }
        let unlit = self.light_view == Mode::Off;
        match target {
            Target::Object(
                Kind::Creature
                | Kind::Waypoint
                | Kind::Sound
                | Kind::Store
                | Kind::Encounter
                | Kind::Trigger,
                _,
            )
            | Target::Prop(_)
            | Target::Box(_)
            | Target::Entry => true,
            Target::Object(Kind::Placeable | Kind::Door, _)
            | Target::Room(_)
            | Target::Lamp(_)
            | Target::Light(_) => unlit,
            // The sun lights everything: the area is drawn again.
            Target::Fitted(_) | Target::Sun => false,
        }
    }

    /// Keeps the scene as it is drawn after an edit that `follows`, when
    /// every change since the drawing was one (`followed`).
    fn followed_to(&mut self, before: u64, follows: bool) {
        let chained = before == self.built || before == self.followed;
        if follows && chained {
            self.followed = self.editor.changes();
        }
    }

    fn area_index(ir: &ModuleIr, area: &str) -> Option<usize> {
        ir.areas
            .iter()
            .position(|candidate| candidate.resref.eq_ignore_ascii_case(area))
    }

    /// A new map's module as it is now (saved or not): its `module.ifo`,
    /// its area's ARE, layout and visibility, and its rooms' copies, made
    /// from the install's rooms.
    fn map_files(&mut self) -> Vec<(String, ResType, Vec<u8>)> {
        let project = &self.editor.project;
        let Some(map) = &project.map else {
            return Vec::new();
        };
        let (module, area) = (project.module.clone(), project.area.clone());
        let mut files = vec![
            (
                "module".to_string(),
                ResType::IFO,
                map.ifo(&module, &area).write(),
            ),
            (area.clone(), ResType::ARE, project.map_are(map).write()),
            (area.clone(), ResType::LYT, map.layout(&module).write()),
            (area.clone(), ResType::VIS, map.visibility(&module).write()),
        ];
        let rooms = map.rooms.clone();
        let mut problems = Vec::new();
        let mut used = std::collections::BTreeSet::new();
        for (index, room) in rooms.iter().enumerate() {
            let name = level_edit::map::room_name(&module, index);
            let by: [f32; 3] = std::array::from_fn(|axis| room.position[axis] - room.from[axis]);
            let key = (
                name.clone(),
                room.model.clone(),
                [by[0], by[1], by[2], room.yaw].map(f32::to_bits),
            );
            if !self.made_rooms.contains_key(&key) {
                let resolver = &self.setup.resolver;
                let read = |name: &str, restype: ResType| resolver.read(name, restype);
                match level_edit::map::room_copy(room, &name, &read) {
                    Ok(made) => {
                        self.made_rooms.insert(key.clone(), made);
                    }
                    Err(why) => {
                        problems.push(why);
                        continue;
                    }
                }
            }
            files.extend(self.made_rooms[&key].iter().cloned());
            used.insert(key);
        }
        // A copy of a room where it no longer stands is dropped.
        self.made_rooms.retain(|key, _| used.contains(key));
        for problem in problems {
            self.say(problem);
        }
        files
    }

    /// The resolver with the edited GIT, the project's templates and a new
    /// map's module as they are now (saved or not) over the project's
    /// folder.
    fn resolver(&mut self) -> Resolver {
        let held = |bytes: Vec<u8>| Location {
            path: PathBuf::from("editor"),
            offset: 0,
            size: bytes.len() as u64,
            held: Some(bytes.into()),
        };
        let mut resources = BTreeMap::new();
        resources.insert(
            (self.editor.project.area.clone(), ResType::GIT),
            held(self.editor.project.git.write()),
        );
        for (resref, made) in &self.editor.project.templates {
            resources.insert((resref.clone(), made.restype), held(made.gff.write()));
        }
        // An installed area's ARE as the project changes it.
        if self.editor.project.map.is_none()
            && let Some(are) = &self.editor.project.are
        {
            resources.insert(
                (self.editor.project.area.clone(), ResType::ARE),
                held(are.write()),
            );
        }
        for (resref, restype, bytes) in self.map_files() {
            resources.insert((resref, restype), held(bytes));
        }
        self.setup.resolver.with_layer(Layer {
            name: "editor".into(),
            rank: Rank::Directory,
            resources,
        })
    }

    /// A template as the object would be made from it: the project's own
    /// (saved or not), else the install's.
    pub(crate) fn template_gff(
        &self,
        resref: &str,
        restype: ResType,
    ) -> Option<fmt_odyssey::gff::Gff> {
        if let Some(made) = self.editor.project.templates.get(resref) {
            return Some(made.gff.clone());
        }
        let read = || {
            self.setup
                .resolver
                .read(resref, restype)
                .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        };
        // The panels ask for the selected object's template every frame.
        let Ok(mut kept) = self.install_templates.lock() else {
            return read();
        };
        kept.entry((resref.to_ascii_lowercase(), restype))
            .or_insert_with(read)
            .clone()
    }

    /// What says what an object does: its template for a kind made from
    /// one (the project's copy, else the install's), its own GIT entry for
    /// a kind read whole from it (a door, a waypoint).
    pub(crate) fn behaviour_source(
        &self,
        kind: Kind,
        entry: &Entry,
    ) -> Option<fmt_odyssey::gff::Gff> {
        match level_edit::template::restype(kind) {
            Some(restype) => self.template_gff(&entry.template, restype),
            None => {
                level_edit::git::item(&self.editor.project.git, kind, entry.index).map(|item| {
                    fmt_odyssey::gff::Gff {
                        file_type: *b"GIT ",
                        root: item.clone(),
                    }
                })
            }
        }
    }

    /// Makes `change` in the template of `target` (`Editor::change_template`).
    pub(crate) fn change_template(&mut self, target: Target, change: Change) {
        let resolver = &self.setup.resolver;
        let read = |name: &str, restype: ResType| resolver.read(name, restype);
        let taken = |name: &str| {
            level_edit::template::TYPES
                .iter()
                .any(|(restype, _)| resolver.find(name, *restype).is_some())
        };
        if let Err(why) = self
            .editor
            .change_template(target, &change, (&read, &taken))
        {
            self.refuse(why);
        }
        self.quiet = 0;
    }

    /// The edited area's collision, with the project's boxes and solid
    /// props, built again when the project changed since.
    pub(crate) fn collision(&mut self) -> Option<&collision_odyssey::ModuleCollision> {
        let changes = self.editor.changes();
        if self.collision.as_ref().is_none_or(|(at, _)| *at != changes) {
            let resolver = self.resolver();
            let (added, _) =
                level_edit::active::collision_of(&self.editor.project, self.setup.ja.as_ref());
            let game = self.setup.game.key();
            let module = self.editor.project.module.clone();
            let clip = match collision_odyssey::built_in_clip(game, &module) {
                Ok(clip) => clip,
                Err(why) => {
                    self.say(why);
                    None
                }
            };
            let clip = level_edit::active::with_added(clip, &added);
            match collision_odyssey::build_module(
                &ResolverSource(&resolver),
                &module,
                Some(&clip),
                &collision_odyssey::BuildOptions::default(),
            ) {
                Ok(mut built) => {
                    built.set_doors(collision_odyssey::Doors::Openable);
                    self.collision = Some((changes, built));
                }
                Err(why) => {
                    self.say(format!("the collision does not build: {why}"));
                    return None;
                }
            }
        }
        self.collision.as_ref().map(|(_, built)| built)
    }

    /// The tag an entry goes by: its own, else its template's.
    pub(crate) fn tag_of(&self, entry: &Entry) -> String {
        if !entry.tag.is_empty() {
            return entry.tag.clone();
        }
        let (_, restype) = entry.kind.list_and_type();
        self.template_gff(&entry.template, restype)
            .and_then(|gff| {
                gff.root
                    .bytes("Tag")
                    .map(|tag| String::from_utf8_lossy(tag).into_owned())
            })
            .unwrap_or_else(String::new)
    }
}

/// A flat colour as a 4 by 4 texture.
fn colour_texture(rgba: [u8; 4]) -> TextureIr {
    TextureIr {
        width: 4,
        height: 4,
        encoding: Encoding::Rgba8,
        mips: vec![rgba.repeat(16)],
        cube: false,
        frames: 1,
        facts: TextureFacts {
            alpha: rgba[3] < 255,
            two_sided: true,
            ..TextureFacts::default()
        },
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}

/// A model of one node and one mesh of `triangles` in its own space,
/// coloured by `texture`.
fn flat_model(name: &str, triangles: &[[[f32; 3]; 3]], texture: &str, glow: bool) -> ModelIr {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    for [a, b, c] in triangles {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = Vec3::from_array([
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ])
        .normalize_or(Vec3::Z)
        .to_array();
        positions.extend([*a, *b, *c]);
        normals.extend([n; 3]);
    }
    let count = positions.len();
    ModelIr {
        name: name.to_string(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: name.to_string(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes: vec![MeshIr {
            node: 0,
            positions,
            normals,
            uv1: vec![[0.5, 0.5]; count],
            uv2: Vec::new(),
            indices: (0..count as u32).collect(),
            texture: Some(texture.to_string()),
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            shadow: false,
            self_illum: if glow { [1.0; 3] } else { [0.0; 3] },
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
            dangly: None,
        }],
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// A light's marker texture: its name and its colour, the colour scaled so
/// its strongest channel is full.
pub(crate) fn light_texture(colour: [f32; 3]) -> (String, [u8; 4]) {
    let most = colour.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-3);
    let [r, g, b] = colour.map(|value| ((value / most).clamp(0.0, 1.0) * 255.0) as u8);
    (
        format!("kjk-editor-light-{r:02x}{g:02x}{b:02x}"),
        [r, g, b, 255],
    )
}

/// The marker colour of each kind.
pub(crate) fn kind_colour(kind: Kind) -> (&'static str, [u8; 4]) {
    match kind {
        Kind::Creature => ("kjk-editor-creature", [255, 80, 80, 255]),
        Kind::Door => ("kjk-editor-door", [170, 120, 255, 255]),
        Kind::Placeable => ("kjk-editor-placeable", [255, 190, 60, 255]),
        Kind::Trigger => ("kjk-editor-trigger", [80, 220, 255, 255]),
        Kind::Waypoint => ("kjk-editor-waypoint", [90, 255, 120, 255]),
        Kind::Sound => ("kjk-editor-sound", [255, 120, 220, 255]),
        Kind::Encounter => ("kjk-editor-encounter", [255, 255, 255, 255]),
        Kind::Store => ("kjk-editor-store", [255, 255, 120, 255]),
    }
}

pub(crate) const PROP_MARKER: (&str, [u8; 4]) = ("kjk-editor-prop", [120, 200, 255, 255]);
/// A lamp placed by hand: warm yellow.
pub(crate) const LAMP_MARKER: (&str, [u8; 4]) = ("kjk-editor-lamp", [255, 210, 90, 255]);
pub(crate) const ENTRY_MARKER: (&str, [u8; 4]) = ("kjk-editor-entry", [255, 230, 60, 255]);
const BOX_FILL: (&str, [u8; 4]) = ("kjk-editor-box", [60, 230, 255, 110]);
const TRIGGER_FILL: (&str, [u8; 4]) = ("kjk-editor-trigger-fill", [80, 220, 255, 70]);
const ENCOUNTER_FILL: (&str, [u8; 4]) = ("kjk-editor-encounter-fill", [255, 255, 255, 60]);
const WALK_FILL: (&str, [u8; 4]) = ("kjk-editor-walkable", [60, 255, 90, 90]);
const WALKABLE_BOX: (&str, [u8; 4]) = ("kjk-editor-walkable-box", [80, 255, 170, 110]);
const NAV_BLOCK_BOX: (&str, [u8; 4]) = ("kjk-editor-nav-block", [255, 60, 60, 90]);
const NAV_FLOOR_BOX: (&str, [u8; 4]) = ("kjk-editor-nav-floor", [120, 255, 60, 90]);
/// The navigation mesh's islands, each its own colour in turn.
const NAV_ISLANDS: [(&str, [u8; 4]); 6] = [
    ("kjk-editor-nav-0", [70, 200, 255, 120]),
    ("kjk-editor-nav-1", [255, 210, 60, 120]),
    ("kjk-editor-nav-2", [190, 110, 255, 120]),
    ("kjk-editor-nav-3", [60, 255, 160, 120]),
    ("kjk-editor-nav-4", [255, 140, 200, 120]),
    ("kjk-editor-nav-5", [255, 255, 255, 120]),
];
/// A face under furniture, and one under a door or a placeable.
const NAV_COVERED: (&str, [u8; 4]) = ("kjk-editor-nav-covered", [200, 30, 30, 150]);
const NAV_HELD: (&str, [u8; 4]) = ("kjk-editor-nav-held", [130, 40, 160, 150]);
const SOLID_FILL: (&str, [u8; 4]) = ("kjk-editor-solid", [255, 130, 60, 60]);
const WALL_FILL: (&str, [u8; 4]) = ("kjk-editor-wall", [90, 120, 255, 80]);

/// The scene of the project as it is now.
fn draw(state: &mut EditorState) -> Result<LoadedScene, String> {
    let started = std::time::Instant::now();
    // The lighting the scene is drawn with: the view's mode, its bake, and
    // the lights placed by hand as the project has them now.
    let module = state.editor.project.module.clone();
    kjk_game::fitted_lights::set_mode(state.light_view);
    kjk_game::viewer::set_bake_folder(match state.light_view {
        kjk_game::fitted_lights::Mode::Baked => state.bake.clone(),
        _ => None,
    });
    kjk_game::fitted_lights::set_placed(
        &module,
        Some(
            state
                .editor
                .project
                .kjk
                .lights
                .iter()
                .map(kjk_game::fitted_lights::Light::of_placed)
                .collect(),
        ),
    );
    kjk_game::lamps::set_lamps(&module, Some(state.editor.project.kjk.lamps.clone()));
    kjk_game::sun::set_sun(&module, Some(state.editor.project.kjk.sun));
    // The kinds of fitting changed, and the ones changed back.
    let mut kinds = String::new();
    for (key, original) in &state.kind_originals {
        let kind = (state.editor.project.kinds)
            .iter()
            .find(|kind| (kind.texture.clone(), kind.placeable) == *key)
            .unwrap_or(original);
        kinds.push_str(&level_edit::kinds::kind_entry(kind));
        kinds.push('\n');
    }
    if !kinds.is_empty()
        && let Err(why) =
            kjk_game::fitted_lights::set_kinds(&state.editor.project.game.clone(), &kinds)
    {
        state.say(format!("the kinds changed do not read: {why}"));
    }
    let resolver = state.resolver();
    let ir = ModuleIr::build(&module, &ResolverSource(&resolver))
        .map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = EditorState::area_index(&ir, &state.editor.project.area).ok_or(format!(
        "{module} has no area {}",
        state.editor.project.area
    ))?;
    let mut scene = kjk_game::viewer::module_scene(&resolver, state.setup.game, &ir, area)?;
    // The game's sun (`kjk_game::sun::found`) and the middle of the level's
    // floors, which the sun is turned round (`Target::Sun`).
    state.editor.game_sun = kjk_game::sun::found(&scene.models);
    let (mut low, mut high) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for corner in scene
        .rooms
        .iter()
        .flat_map(|room| room.floor.iter().flatten())
    {
        for axis in 0..3 {
            low[axis] = low[axis].min(corner[axis]);
            high[axis] = high[axis].max(corner[axis]);
        }
    }
    if low.iter().chain(&high).all(|value| value.is_finite()) {
        state.editor.level_middle = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    }
    let mut drawn = Drawn::default();

    // The objects' models: a door's or placeable's by its object number,
    // a creature's body. The module reader leaves out a creature it cannot
    // make, so entries are matched to objects in order, kind by kind, by
    // template.
    let instances = &ir.areas[area].instances;
    let entries = state.editor.entries();
    let mut models_of: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut next = 0usize;
    for (object, instance) in instances.iter().enumerate() {
        let found = (next..entries.len()).find(|&at| {
            entries[at].kind == instance.kind
                && entries[at]
                    .template
                    .eq_ignore_ascii_case(&instance.template)
        });
        let Some(at) = found else { continue };
        next = at + 1;
        if let Some(model) = scene.instance_models.get(&(object as u32)) {
            models_of[at].push(*model);
        }
        for view in scene
            .creature_views
            .iter()
            .filter(|view| view.object == object as u32)
        {
            models_of[at].push(view.body);
        }
    }

    // Props.
    let files = state
        .setup
        .ja
        .as_ref()
        .map(kjk_game::viewer_ja::Files::base);
    let props = state.editor.project.kjk.props.clone();
    let (prop_models, notes) =
        project_props::add(&mut scene, files.as_ref(), &mut state.prop_models, &props);
    drop(files);
    for note in notes {
        state.say(note);
    }

    // Boxes and outlines. The markers are icons over the view (`markers`).
    let mut textures: Vec<(&str, [u8; 4])> = vec![
        BOX_FILL,
        WALKABLE_BOX,
        NAV_BLOCK_BOX,
        NAV_FLOOR_BOX,
        TRIGGER_FILL,
        ENCOUNTER_FILL,
    ];
    for (entry, models) in entries.iter().zip(models_of) {
        drawn
            .things
            .push((Target::Object(entry.kind, entry.index), models));
    }
    // A trigger's and an encounter's outline: a see-through floor of its
    // shape, a few centimetres up, moved with the object.
    for (entry_at, entry) in entries.iter().enumerate() {
        if !matches!(entry.kind, Kind::Trigger | Kind::Encounter) {
            continue;
        }
        let outline = level_edit::git::outline(&state.editor.project.git, entry.kind, entry.index);
        if outline.len() < 3 {
            continue;
        }
        let lift = |p: [f32; 3]| [p[0], p[1], p[2] + 0.05];
        let fan: Vec<[[f32; 3]; 3]> = (1..outline.len() - 1)
            .map(|at| [lift(outline[0]), lift(outline[at]), lift(outline[at + 1])])
            .collect();
        let texture = if entry.kind == Kind::Trigger {
            TRIGGER_FILL.0
        } else {
            ENCOUNTER_FILL.0
        };
        let model = scene.models.len();
        scene.models.push(Placed::at(
            flat_model("outline", &fan, texture, true),
            entry.position,
        ));
        if let Some((_, models)) = drawn.things.get_mut(entry_at) {
            models.push(model);
        }
    }
    for (at, model) in prop_models.into_iter().enumerate() {
        drawn
            .things
            .push((Target::Prop(at), model.into_iter().collect()));
    }
    // The lamps placed by hand: the scene drew their models
    // (`kjk_game::lamps`), in the order the project lists them.
    let lamps = state.editor.project.kjk.lamps.clone();
    for (at, lamp) in lamps.iter().enumerate() {
        let models = scene.lamps.get(at).and_then(|(model, _)| *model);
        if models.is_none() {
            state.say(format!(
                "lamp {at} ({}) is not drawn: its model does not read",
                lamp.label()
            ));
        }
        drawn
            .things
            .push((Target::Lamp(at), models.into_iter().collect()));
    }
    for (at, shape) in state.editor.project.kjk.boxes.iter().enumerate() {
        let local = CollisionBox {
            centre: [0.0; 3],
            yaw: 0.0,
            ..shape.clone()
        };
        let fill = match shape.role {
            BoxRole::Solid => BOX_FILL.0,
            BoxRole::Walkable => WALKABLE_BOX.0,
            BoxRole::NavBlock => NAV_BLOCK_BOX.0,
            BoxRole::NavFloor => NAV_FLOOR_BOX.0,
        };
        let model = scene.models.len();
        scene.models.push(Placed {
            facing: shape.yaw.to_radians(),
            ..Placed::at(
                flat_model("box", &local.triangles(), fill, true),
                shape.centre,
            )
        });
        drawn.things.push((Target::Box(at), vec![model]));
    }

    // A new map's rooms, in the layout's order, and its entry.
    if let Some(map) = &state.editor.project.map {
        for at in 0..map.rooms.len() {
            let models = scene
                .rooms
                .get(at)
                .and_then(|room| room.model)
                .into_iter()
                .collect();
            drawn.things.push((Target::Room(at), models));
            drawn.room_yaws.push(map.rooms[at].yaw);
        }
    }

    // The collision overlay.
    if state.show_collision {
        textures.extend([WALK_FILL, SOLID_FILL, WALL_FILL]);
        if let Some(built) = state.collision() {
            let mut walk = Vec::new();
            let mut solid = Vec::new();
            let mut walls = Vec::new();
            let lift = |t: &[[f32; 3]; 3]| t.map(|[x, y, z]| [x, y, z + 0.02]);
            for triangle in &built.collision.triangles {
                match triangle.origin {
                    collision_odyssey::Origin::Walkmesh
                        if built.surfaces.walkable(triangle.material) =>
                    {
                        walk.push(lift(&triangle.corners));
                    }
                    collision_odyssey::Origin::EdgeWall => walls.push(triangle.corners),
                    _ => solid.push(triangle.corners),
                }
            }
            let mut overlay = flat_model("collision", &walk, WALK_FILL.0, true);
            for (triangles, texture) in [(solid, SOLID_FILL.0), (walls, WALL_FILL.0)] {
                let mut part = flat_model("collision", &triangles, texture, true);
                overlay.meshes.append(&mut part.meshes);
            }
            drawn.overlay = Some(scene.models.len());
            scene.models.push(Placed::at(overlay, [0.0; 3]));
        }
    }
    // A bake asked for starts from the scene's lights, before the editor's
    // markers join the scene.
    if let Some(label) = state.bake_request.take() {
        match start_bake(state, &scene, label) {
            Ok(job) => state.bake_job = Some(job),
            Err(why) => state.say(format!("the bake did not start: {why}")),
        }
    }

    // Each room's level, measured once, from the stored lightmaps.
    if state.show_levels
        && state.room_measures.is_none()
        && state.light_view != kjk_game::fitted_lights::Mode::Baked
    {
        state.room_measures = Some(measure_rooms(&scene));
    }

    // The lights the game places itself, read from the scene as play
    // places them (`fitted_lights::explained`). Each light's marker is an
    // icon over the view (`markers`), and the selected one's reach is drawn
    // there too (`ui::reach_in_view`).
    state.editor.seen_lights.clear();
    state.editor.seen_on_placeable.clear();
    if state.show_lights {
        let found = kjk_game::fitted_lights::explained(&scene);
        let fitted: Vec<_> = found
            .iter()
            .filter(|(_, source, _)| source != "placed")
            .collect();
        state.editor.seen_lights = fitted
            .iter()
            .map(|(light, source, _)| (light.placed(), source.clone()))
            .collect();
        state.editor.seen_on_placeable = fitted.iter().map(|(_, _, on)| *on).collect();
    }
    state.shown_lights = (state.light_view, state.bake.clone(), state.show_lights);
    state.creature_talks = crate::markers::creature_talks(state);

    // The navigation mesh, as play bakes it with the project's changes
    // (`level_edit::nav`).
    state.nav_facts = None;
    if state.show_nav {
        textures.extend(NAV_ISLANDS);
        textures.extend([NAV_COVERED, NAV_HELD]);
        let boxes = state.editor.project.kjk.boxes.clone();
        let entry = state.editor.project.map.as_ref().map(|map| map.entry);
        let wanted: Vec<_> = (state.editor.project.kjk.links.iter())
            .map(|link| (link.kind, link.from, link.to))
            .collect();
        if let Some(built) = state.collision() {
            let floor = level_edit::nav::edited(built.collision.nav_faces(), &boxes);
            let mut nav =
                collision::nav::NavMesh::build(&floor, sim::walk::CLEARANCE * collision::UNIT);
            nav.mark(&built.collision.level);
            // The links between floors, as play adds them, each checked as
            // the containment audit moves a player: a plain jump, or a drop.
            let mut checks: Vec<Result<(), String>> = vec![Ok(()); wanted.len()];
            for (at, why) in nav.add_links(&wanted) {
                checks[at] = Err(why);
            }
            for (check, (kind, from, to)) in checks.iter_mut().zip(&wanted) {
                if check.is_ok() {
                    *check = collision::jumps::link_lands(
                        &built.collision.level,
                        (*from, *to),
                        collision::jumps::PLAIN_JUMP,
                        *kind == collision::nav::LinkKind::Drop,
                    );
                }
            }
            let mut sizes = vec![0usize; nav.islands];
            for face in &nav.faces {
                sizes[face.island as usize] += 1;
            }
            let largest = (0..sizes.len()).max_by_key(|&island| sizes[island]);
            let at_entry = entry
                .and_then(|entry| nav.locate(entry, 1.0, 1.0))
                .map(|face| nav.faces[face as usize].island);
            let entry_off = entry.is_some() && at_entry.is_none();
            let (main, from) = match at_entry {
                Some(island) => (Some(island), "the entry"),
                None => (largest.map(|island| island as u32), "the largest island"),
            };
            let mut groups: BTreeMap<&str, Vec<[[f32; 3]; 3]>> = BTreeMap::new();
            for face in &nav.faces {
                let texture = if face.covered {
                    NAV_COVERED.0
                } else if face.entity.is_some() {
                    NAV_HELD.0
                } else {
                    NAV_ISLANDS[face.island as usize % NAV_ISLANDS.len()].0
                };
                groups.entry(texture).or_default().push(
                    face.corners
                        .map(|corner| [corner[0], corner[1], corner[2] + 0.04]),
                );
            }
            let mut overlay = flat_model("nav", &[], NAV_ISLANDS[0].0, true);
            overlay.meshes.clear();
            for (texture, triangles) in groups {
                let mut part = flat_model("nav", &triangles, texture, true);
                overlay.meshes.append(&mut part.meshes);
            }
            scene.models.push(Placed::at(overlay, [0.0; 3]));
            // The islands the links lead to from the main one.
            let mut reached: std::collections::BTreeSet<u32> = main.into_iter().collect();
            loop {
                let more: Vec<u32> = (nav.links.iter())
                    .flat_map(|link| {
                        reached
                            .iter()
                            .filter_map(|island| link.crossed_from(&nav, *island))
                            .filter_map(|(_, land)| nav.island_at(land, 0.5, 0.5))
                            .collect::<Vec<_>>()
                    })
                    .filter(|island| !reached.contains(island))
                    .collect();
                if more.is_empty() {
                    break;
                }
                reached.extend(more);
            }
            let apart_of = |counted: &dyn Fn(u32) -> bool| {
                nav.faces
                    .iter()
                    .filter(|face| !counted(face.island))
                    .count()
            };
            let apart = apart_of(&|island| Some(island) == main);
            state.nav_facts = Some(NavFacts {
                faces: nav.faces.len(),
                islands: nav.islands,
                portals: nav.portals(),
                from,
                apart,
                entry_off,
                links: checks,
                by_links: apart - apart_of(&|island| reached.contains(&island)),
            });
        }
    }
    for (name, rgba) in textures {
        scene
            .textures
            .insert(name.to_string(), colour_texture(rgba));
    }
    state.drawn = drawn;
    state.built = state.editor.changes();
    state.followed = u64::MAX;
    state.shown_collision = state.show_collision;
    state.shown_nav = state.show_nav;
    state.redraws += 1;
    info!(
        target: "editor",
        "drew {module} in {:.0} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(scene)
}

/// The scene drawn again once the edits have stopped for a moment.
pub(crate) fn redraw(mut commands: Commands, mut state: ResMut<EditorState>) {
    state.quiet = state.quiet.saturating_add(1);
    crate::recover::tick(&mut state);
    // A bake that ended is shown.
    let ended = state.bake_job.as_mut().and_then(crate::bake::BakeJob::poll);
    if let Some(ended) = ended {
        let job = state.bake_job.take().expect("polled above");
        let seconds = job.started.elapsed().as_secs();
        match ended {
            Ok(bake) => {
                state.setup.bakes.retain(|had| *had != bake);
                state.setup.bakes.insert(0, bake.clone());
                state.bake = Some(bake);
                state.light_view = kjk_game::fitted_lights::Mode::Baked;
                state.say(format!(
                    "bake {} done in {seconds} s: {}",
                    job.label, job.status
                ));
            }
            Err(why) => state.say(format!(
                "bake {} failed after {seconds} s: {why}",
                job.label
            )),
        }
    }
    // An audit that ended is shown.
    let audited = (state.audit_job.as_ref())
        .and_then(|(_, result)| result.lock().ok().and_then(|mut slot| slot.take()));
    if let Some(audited) = audited {
        let seconds =
            (state.audit_job.take()).map_or(0, |(started, _)| started.elapsed().as_secs());
        match audited {
            Ok(output) => finish_audit(&mut state, &output, seconds),
            Err(why) => state.say(why),
        }
    }
    // A try that ended is shown.
    let tried = state
        .script_try
        .as_ref()
        .and_then(crate::script_edit::Try::poll);
    if let Some(tried) = tried {
        state.script_try = None;
        match tried {
            Ok(outcome) => {
                let how = if outcome.ok {
                    "ran"
                } else {
                    "did not run cleanly"
                };
                state.say(format!("{} {how}", outcome.script));
                state.tried = Some(outcome);
            }
            Err(why) => state.say(why),
        }
    }
    let asked = state.bake_request.is_some();
    // The Room levels window measures the rooms when a scene is drawn.
    let measure = state.show_levels
        && state.room_measures.is_none()
        && state.light_view != kjk_game::fitted_lights::Mode::Baked;
    let stale = measure
        || asked
        || state.built != state.editor.changes()
        || state.shown_collision != state.show_collision
        || state.shown_nav != state.show_nav
        || state.shown_lights != (state.light_view, state.bake.clone(), state.show_lights);
    if !stale || state.quiet < SETTLE_FRAMES || state.dragging {
        return;
    }
    // Only moved or turned things since the drawing, which `follow` has put
    // where they are: the scene stays, and letting go of a drag does not
    // stop the editor while the whole area is drawn again.
    let others_stale = measure
        || asked
        || state.shown_collision != state.show_collision
        || state.shown_nav != state.show_nav
        || state.shown_lights != (state.light_view, state.bake.clone(), state.show_lights);
    if !others_stale && state.followed == state.editor.changes() {
        state.built = state.editor.changes();
        state.followed = u64::MAX;
        return;
    }
    match draw(&mut state) {
        Ok(scene) => commands.insert_resource(SceneContent(Box::new(scene))),
        Err(why) => {
            state.say(format!("the area does not draw: {why}"));
            state.built = state.editor.changes();
        }
    }
}

/// Each drawn thing's models and marker moved to where the project has it
/// now: an edit shows before the scene is drawn again.
pub(crate) fn follow(
    state: Res<EditorState>,
    scene: Option<Res<SceneContent>>,
    mut roots: Query<(&SceneModel, &mut Transform)>,
) {
    let Some(scene) = scene else { return };
    let mut wanted: BTreeMap<usize, Mat4> = BTreeMap::new();
    for (target, models) in &state.drawn.things {
        let Some((position, facing)) = state.editor.placement(*target) else {
            continue;
        };
        // A room's copy was drawn turned already: it turns by what it has
        // turned since.
        let facing = match target {
            Target::Room(at) => facing - state.drawn.room_yaws.get(*at).copied().unwrap_or(facing),
            _ => facing,
        };
        // A lamp's pitch and roll as it has them now, not as it was drawn.
        let tilt = match target {
            Target::Lamp(at) => Some(
                state
                    .editor
                    .project
                    .kjk
                    .lamps
                    .get(*at)
                    .and_then(kjk_game::lamps::tilt),
            ),
            _ => None,
        };
        for &model in models {
            let Some(placed) = scene.0.models.get(model) else {
                continue;
            };
            if placed.hook.is_some() {
                continue;
            }
            let frame = match &tilt {
                Some(attach) => root_attached(attach, position, facing),
                None => root(placed, position, facing),
            };
            wanted.insert(model, frame);
        }
    }
    for (model, mut transform) in &mut roots {
        if let Some(matrix) = wanted.get(&model.index) {
            let next = Transform::from_matrix(*matrix);
            if *transform != next {
                *transform = next;
            }
        }
    }
}

/// Where a placed model's root sits when its object stands at `position`
/// facing `facing` degrees: as `scene::placement` places it.
pub(crate) fn root(placed: &Placed, position: [f32; 3], facing: f32) -> Mat4 {
    root_attached(&placed.attach, position, facing)
}

/// `root` for a model attached by `attach`, whatever the model is.
fn root_attached(
    attach: &Option<asset_odyssey::model::Matrix>,
    position: [f32; 3],
    facing: f32,
) -> Mat4 {
    let frame = Mat4::from_rotation_translation(
        Quat::from_rotation_z(facing.to_radians()),
        Vec3::from_array(position),
    );
    match attach {
        Some(attach) => frame * kjk_game::scene::to_mat4(attach),
        None => frame,
    }
}

/// A ray from the camera through a point of its view (pixels), in game
/// space.
pub(crate) fn ray_at(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    point: Vec2,
) -> Option<(Vec3, Vec3)> {
    let (camera, transform) = cameras.iter().next()?;
    let ray = camera.viewport_to_world(transform, point).ok()?;
    let to_game = game_to_bevy().inverse();
    Some((to_game * ray.origin, to_game * Vec3::from(ray.direction)))
}

/// The centre of the camera's view, pixels.
pub(crate) fn view_centre(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Option<Vec2> {
    let (camera, _) = cameras.iter().next()?;
    camera.logical_viewport_size().map(|size| size / 2.0)
}

/// Where a ray first meets the level, if within 200 m.
pub(crate) fn ground(
    state: &mut EditorState,
    (origin, direction): (Vec3, Vec3),
) -> Option<[f32; 3]> {
    use collision::CollisionWorld;
    let end = origin + direction.normalize_or_zero() * 200.0;
    let built = state.collision()?;
    let trace = built.collision.level.trace(
        origin.to_array(),
        end.to_array(),
        &collision::Extents::POINT,
        collision::Contents::PLAYER_SOLID,
    );
    (trace.fraction < 1.0 && !trace.start_solid).then_some(trace.end)
}

/// How high each surface of the level is at `x` `y` (metres), from the top
/// down: a roof, then the floors under it. At most 16 are found, within
/// 200 m above and below the level's middle. `None` when the collision
/// does not build.
pub(crate) fn surfaces_at(state: &mut EditorState, x: f32, y: f32) -> Option<Vec<f32>> {
    use collision::CollisionWorld;
    let middle = state.editor.level_middle[2];
    let (top, bottom) = (middle + 200.0, middle - 200.0);
    let built = state.collision()?;
    let mut found: Vec<f32> = Vec::new();
    let mut from = top;
    while from > bottom && found.len() < 16 {
        let trace = built.collision.level.trace(
            [x, y, from],
            [x, y, bottom],
            &collision::Extents::POINT,
            collision::Contents::PLAYER_SOLID,
        );
        // Inside something solid: on down until the point is free.
        if trace.start_solid {
            from -= 0.25;
            continue;
        }
        if trace.fraction >= 1.0 {
            break;
        }
        let z = trace.end[2];
        if found.last().is_none_or(|last| (last - z).abs() > 0.05) {
            found.push(z);
        }
        from = z - 0.05;
    }
    Some(found)
}

/// How high the walkable floor is at `x` `y` (metres), from the top down:
/// each walkable face of the rooms' walkmeshes there, and the top of each
/// of the project's boxes that are walked on (but the box `but`, which is
/// the one being dropped). Furniture, walls and roofs are left out.
pub(crate) fn walkable_at(state: &mut EditorState, x: f32, y: f32, but: Option<usize>) -> Vec<f32> {
    let mut faces: Vec<[[f32; 3]; 3]> = Vec::new();
    for (at, shape) in state.editor.project.kjk.boxes.iter().enumerate() {
        if Some(at) != but && matches!(shape.role, BoxRole::Walkable | BoxRole::NavFloor) {
            faces.extend(shape.top());
        }
    }
    if let Some(built) = state.collision() {
        for triangle in &built.collision.triangles {
            if matches!(triangle.origin, collision_odyssey::Origin::Walkmesh)
                && built.surfaces.walkable(triangle.material)
            {
                faces.push(triangle.corners);
            }
        }
    }
    let mut found: Vec<f32> = Vec::new();
    for [a, b, c] in faces {
        // Where the point lies in the face, seen from above.
        let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if d.abs() < 1e-9 {
            continue;
        }
        let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
        let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
        let w = 1.0 - u - v;
        if u >= -1e-4 && v >= -1e-4 && w >= -1e-4 {
            found.push(u * a[2] + v * b[2] + w * c[2]);
        }
    }
    found.sort_by(|a, b| b.total_cmp(a));
    found.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    found
}

/// Puts everything selected on the walkable floor under it, as one edit:
/// each thing goes straight down to the highest floor at or under it, or,
/// with none under it, up to the lowest above. A box rests its underside
/// on the floor. The sun, the game's own lights and a new map's rooms stay.
fn drop_to_floor(state: &mut EditorState) -> Result<(), String> {
    let selection = state.editor.selection();
    if selection.is_empty() {
        return Err("nothing is selected".into());
    }
    let mut edits = Vec::new();
    for target in selection {
        if matches!(target, Target::Sun | Target::Fitted(_) | Target::Room(_)) {
            continue;
        }
        let Some((at, _)) = state.editor.placement(target) else {
            continue;
        };
        let (lift, own) = match target {
            Target::Box(n) => (
                (state.editor.project.kjk.boxes.get(n)).map_or(0.0, |shape| shape.size[2] * 0.5),
                Some(n),
            ),
            _ => (0.0, None),
        };
        let floors = walkable_at(state, at[0], at[1], own);
        let under = at[2] - lift + 0.5;
        let floor = (floors.iter().copied().find(|z| *z <= under)).or(floors.last().copied());
        if let Some(floor) = floor
            && (floor + lift - at[2]).abs() > 1e-3
        {
            edits.push(Edit::Move(target, [at[0], at[1], floor + lift]));
        }
    }
    if edits.is_empty() {
        return Err(
            "nothing selected moved: each thing stands on its floor already, or has no walkable floor under it"
                .into(),
        );
    }
    state.apply_many(edits);
    Ok(())
}

/// The text of the project's script `name` as its folder holds it.
pub(crate) fn script_text_on_disk(state: &EditorState, name: &str) -> Option<String> {
    let bytes = std::fs::read(state.editor.project.folder.join(format!("{name}.nss"))).ok()?;
    Some(bytes.into_iter().map(char::from).collect())
}

/// Where a new thing goes: the level under the screen's centre, else a
/// few metres ahead of the camera.
pub(crate) fn place_point(
    state: &mut EditorState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> [f32; 3] {
    let Some(ray) = view_centre(cameras).and_then(|centre| ray_at(cameras, centre)) else {
        return [0.0; 3];
    };
    ground(state, ray)
        .unwrap_or_else(|| (ray.0 + ray.1.normalize_or_zero() * PLACE_AHEAD_M).to_array())
}

/// The nearest drawn thing a ray passes through: its model's bounds. A
/// marker is picked on the screen before this (`markers::under`). A new map's room is picked last, when nothing else is: the room
/// whose model holds `floor`, where the ray meets the level, and of those
/// the smallest.
pub(crate) fn pick(
    state: &EditorState,
    scene: &LoadedScene,
    (origin, direction): (Vec3, Vec3),
    floor: Option<[f32; 3]>,
) -> Option<Target> {
    let direction = direction.normalize_or_zero();
    let mut best: Option<(f32, Target)> = None;
    let mut offer = |distance: f32, target: Target| {
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, target));
        }
    };
    for (target, models) in &state.drawn.things {
        let Some((position, facing)) = state.editor.placement(*target) else {
            continue;
        };
        if let Target::Room(_) = target {
            continue;
        }
        for &model in models {
            let Some((low, high)) = scene
                .models
                .get(model)
                .and_then(kjk_game::scene::local_bounds)
            else {
                continue;
            };
            // The ray in the model's own space.
            let frame = Mat4::from_rotation_translation(
                Quat::from_rotation_z(facing.to_radians()),
                Vec3::from_array(position),
            )
            .inverse();
            let o = frame.transform_point3(origin);
            let d = frame.transform_vector3(direction);
            let mut near = 0.0f32;
            let mut far = f32::INFINITY;
            let mut hit = true;
            for axis in 0..3 {
                if d[axis].abs() < 1e-6 {
                    if o[axis] < low[axis] || o[axis] > high[axis] {
                        hit = false;
                    }
                    continue;
                }
                let a = (low[axis] - o[axis]) / d[axis];
                let b = (high[axis] - o[axis]) / d[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            if hit && near <= far && far > 0.0 {
                offer(near, *target);
            }
        }
    }
    if best.is_none()
        && let Some(floor) = floor
    {
        let floor = Vec3::from_array(floor);
        let mut smallest: Option<(f32, Target)> = None;
        for (target, models) in &state.drawn.things {
            let (Target::Room(_), Some((position, _))) = (target, state.editor.placement(*target))
            else {
                continue;
            };
            for &model in models {
                let Some((low, high)) = scene
                    .models
                    .get(model)
                    .and_then(kjk_game::scene::local_bounds)
                else {
                    continue;
                };
                let local = floor - Vec3::from_array(position);
                let slack = Vec3::splat(0.05);
                if local.cmpge(low - slack).all() && local.cmple(high + slack).all() {
                    let size = (high - low).x * (high - low).y;
                    if smallest.is_none_or(|(least, _)| size < least) {
                        smallest = Some((size, *target));
                    }
                }
            }
        }
        return smallest.map(|(_, target)| target);
    }
    best.map(|(_, target)| target)
}

/// The walkmesh of the new map's room `at`, moved to where the map places
/// it; `None` for a room the install has no walkmesh of.
pub(crate) fn room_walkmesh(state: &EditorState, at: usize) -> Option<fmt_odyssey::bwm::Walkmesh> {
    let room = state.editor.project.map.as_ref()?.rooms.get(at)?;
    let bytes = state.setup.resolver.read(&room.model, ResType::WOK)?;
    let walkmesh = fmt_odyssey::bwm::Walkmesh::parse(&bytes).ok()?;
    Some(room.placed(&walkmesh))
}

/// Moves the new map's room `at` so that an edge of its floor meets the
/// nearest edge of another room's (`level_edit::map::seam_offset`), as a
/// part of the last edit when `continuing`. False when no seam is near.
pub(crate) fn snap_to_seam(state: &mut EditorState, at: usize, continuing: bool) -> bool {
    let Some(moving) = room_walkmesh(state, at) else {
        return false;
    };
    let rooms = state
        .editor
        .project
        .map
        .as_ref()
        .map_or(0, |map| map.rooms.len());
    let others: Vec<_> = (0..rooms)
        .filter(|&other| other != at)
        .filter_map(|other| room_walkmesh(state, other))
        .collect();
    let Some(by) = level_edit::map::seam_offset(&moving, &others, level_edit::map::SEAM_REACH)
    else {
        return false;
    };
    let Some((position, _)) = state.editor.placement(Target::Room(at)) else {
        return false;
    };
    let to = std::array::from_fn(|axis| position[axis] + by[axis]);
    let edit = Edit::Move(Target::Room(at), to);
    if continuing {
        state.apply_continuing(edit);
    } else {
        state.apply(edit);
    }
    true
}

/// Adds the install's room `model` to the new map so that its floor's
/// middle stands at `at` (`library` says where its own area places it).
pub(crate) fn add_room(state: &mut EditorState, model: &str, at: [f32; 3]) -> Result<(), String> {
    if state.editor.project.map.is_none() {
        return Err("only a new map has rooms (docs/LEVEL-EDIT.md)".into());
    }
    let found = state
        .setup
        .rooms
        .iter()
        .find(|room| room.model.eq_ignore_ascii_case(model))
        .cloned()
        .ok_or(format!("the install has no room {model}"))?;
    // The walkmesh's middle and floor, which stand where the room is put.
    let (middle, low) = state
        .setup
        .resolver
        .read(&found.model, ResType::WOK)
        .and_then(|bytes| fmt_odyssey::bwm::Walkmesh::parse(&bytes).ok())
        .and_then(|walkmesh| {
            let points = walkmesh
                .vertices
                .iter()
                .map(|point| Vec3::from_array(*point));
            kjk_game::scene::span(points)
        })
        .map_or(
            (Vec3::from_array(found.from), found.from[2]),
            |(low, high)| ((low + high) * 0.5, low.z),
        );
    let offset = middle - Vec3::from_array(found.from);
    let position = [
        at[0] - offset.x,
        at[1] - offset.y,
        at[2] - (low - found.from[2]),
    ];
    state.apply(Edit::AddRoom(level_edit::Room {
        model: found.model.clone(),
        from: found.from,
        position,
        yaw: 0.0,
    }));
    Ok(())
}

/// A finite number a command names.
pub(crate) fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(format!("{word:?} is not a number"))
}

pub(crate) fn point(words: &[&str]) -> Result<[f32; 3], String> {
    match words {
        [x, y, z] => Ok([number(x)?, number(y)?, number(z)?]),
        _ => Err("a point is three numbers: x y z".into()),
    }
}

fn kind_named(word: &str) -> Option<Kind> {
    match word {
        "placeable" => Some(Kind::Placeable),
        "creature" => Some(Kind::Creature),
        "door" => Some(Kind::Door),
        "waypoint" => Some(Kind::Waypoint),
        "trigger" => Some(Kind::Trigger),
        "sound" => Some(Kind::Sound),
        "store" => Some(Kind::Store),
        "encounter" => Some(Kind::Encounter),
        _ => None,
    }
}

impl EditSetup {
    /// The install's templates of a kind the editor adds.
    pub(crate) fn templates_of(&self, kind: Kind) -> &[String] {
        match kind {
            Kind::Placeable => &self.placeables,
            Kind::Creature => &self.creatures,
            Kind::Door => &self.doors,
            Kind::Waypoint => &self.waypoints,
            Kind::Trigger => &self.triggers,
            Kind::Sound => &self.sounds,
            Kind::Store => &self.stores,
            Kind::Encounter => &self.encounters,
        }
    }
}

/// Adds an object of `kind` made from the install's `template` at
/// `position`. A kind read whole from its GIT entry (a waypoint) takes the
/// template's own fields into its entry.
pub(crate) fn add_object(
    state: &mut EditorState,
    kind: Kind,
    template: &str,
    position: [f32; 3],
) -> Result<(), String> {
    let template = template.to_ascii_lowercase();
    if !ADDABLE.contains(&kind) {
        return Err(format!("a {} cannot be added", kind_name(kind)));
    }
    if state
        .setup
        .templates_of(kind)
        .binary_search(&template)
        .is_err()
    {
        return Err(format!(
            "the install has no {} template {template}",
            kind_name(kind)
        ));
    }
    let base = if level_edit::template::whole(kind) {
        let (_, restype) = kind.list_and_type();
        let gff = state
            .template_gff(&template, restype)
            .ok_or(format!("the template {template} does not read"))?;
        Some(gff.root)
    } else {
        None
    };
    state.apply(Edit::AddObject {
        kind,
        template,
        position,
        facing: 0.0,
        base,
    });
    Ok(())
}

/// Runs one `edit` command (`docs/LEVEL-EDIT.md`). Keys run the same
/// commands.
pub(crate) fn run(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    if let Err(why) = command(state, words, cameras) {
        // A refused edit has said why already (`EditorState::refuse`).
        if state.messages.last() != Some(&why) {
            state.say(format!("edit {}: {why}", words.join(" ")));
        }
    }
}

/// The object a command names by its kind's word and its place in that
/// kind's list, from 0.
fn object_at(state: &EditorState, kind: &str, n: &str) -> Result<Target, String> {
    let kind = kind_named(kind).ok_or(format!("{kind:?} is not a kind of object"))?;
    let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
    if state.editor.entry(kind, at).is_none() {
        return Err(format!("there is no {} {at}", kind_name(kind)));
    }
    Ok(Target::Object(kind, at))
}

fn selected(state: &EditorState) -> Result<Target, String> {
    state.editor.selected.ok_or("nothing is selected".into())
}

/// Runs one `edit` command and answers how it went: an edit the project
/// refused (a sun's colour below 0, a box too small) is an error here, not
/// only a message.
pub(crate) fn command(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    state.refused = None;
    let done = command_words(state, words, cameras).and_then(|()| match state.refused.take() {
        Some(why) => Err(why),
        None => Ok(()),
    });
    // A command that did nothing says why over the view too.
    if let Err(why) = &done {
        state.refusal = Some((std::time::Instant::now(), why.clone()));
    }
    done
}

fn command_words(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    match words {
        ["select", "prop", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.props.len() {
                return Err(format!("there is no prop {at}"));
            }
            state.editor.select(Some(Target::Prop(at)), false);
        }
        ["select", "box", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.boxes.len() {
                return Err(format!("there is no box {at}"));
            }
            state.editor.select(Some(Target::Box(at)), false);
        }
        ["select", "room", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            let rooms = state
                .editor
                .project
                .map
                .as_ref()
                .map_or(0, |map| map.rooms.len());
            if at >= rooms {
                return Err(format!("there is no room {at}"));
            }
            state.editor.select(Some(Target::Room(at)), false);
        }
        ["select", "sun"] => state.editor.select(Some(Target::Sun), false),
        ["sun", "game"] => state.apply(Edit::SetSun(None)),
        ["sun", field, rest @ ..] => {
            let middle = state.editor.level_middle;
            let mut sun = state
                .editor
                .sun()
                .unwrap_or_else(|| level_edit::area::SunLight {
                    at: [middle[0], middle[1], middle[2] + 500.0],
                    colour: [1.0; 3],
                    multiplier: 1.0,
                    radius: 2000.0,
                });
            let numbers: Vec<f32> = rest
                .iter()
                .map(|word| {
                    word.parse::<f32>()
                        .map_err(|_| format!("{word:?} is not a number"))
                })
                .collect::<Result<_, _>>()?;
            let (bearing, height, distance) = sun.seen_from(middle);
            match (*field, numbers.as_slice()) {
                ("add", []) => {}
                ("bearing", [degrees]) => {
                    sun = sun.placed(
                        middle,
                        (*degrees, height, distance),
                        &collision::trig::sin_cos_deg,
                    );
                }
                ("height", [degrees]) => {
                    sun = sun.placed(
                        middle,
                        (bearing, *degrees, distance),
                        &collision::trig::sin_cos_deg,
                    );
                }
                ("colour", [r, g, b]) => sun.colour = [*r, *g, *b],
                ("multiplier", [value]) => sun.multiplier = *value,
                ("radius", [value]) => sun.radius = *value,
                _ => {
                    return Err(
                        "sun add, sun bearing DEGREES, sun height DEGREES, sun colour R G B, sun multiplier M, sun radius METRES or sun game"
                            .into(),
                    );
                }
            }
            state.apply(Edit::SetSun(Some(sun)));
        }
        ["select", "entry"] => {
            if state.editor.project.map.is_none() {
                return Err("only a new map has an entry".into());
            }
            state.editor.select(Some(Target::Entry), false);
        }
        ["select", "none"] | ["deselect"] => state.editor.select(None, false),
        ["drop"] => drop_to_floor(state)?,
        ["markers", kind, on @ ("on" | "off")] => {
            let kinds: Vec<Kind> = match *kind {
                "all" => Kind::ALL.to_vec(),
                kind => vec![kind_named(kind).ok_or("markers takes a kind of object, or all")?],
            };
            for kind in kinds {
                state.hidden.retain(|hidden| *hidden != kind);
                if *on == "off" {
                    state.hidden.push(kind);
                }
            }
        }
        ["recover"] => crate::recover::take(state)?,
        ["recover", "discard"] => crate::recover::discard(state)?,
        // The recovery copy written now, not at its next turn.
        ["recover", "write"] => {
            if state.recovery.is_some() {
                return Err("a recovery copy waits to be taken or discarded first".into());
            }
            let folder = crate::recover::write(state)?;
            state.say(format!("recovery copy written to {}", folder.display()));
        }
        // An object by its kind and its place in that kind's list, for one
        // whose tag names another object first, or has a space in it.
        ["select", kind, n] if kind_named(kind).is_some() => {
            let target = object_at(state, kind, n)?;
            state.editor.select(Some(target), false);
        }
        ["select", "more", kind, n] if kind_named(kind).is_some() => {
            let target = object_at(state, kind, n)?;
            if !state.editor.selection().contains(&target) {
                state.editor.select(Some(target), true);
            }
        }
        ["select", "more", tag] => {
            let found = state
                .editor
                .entries()
                .into_iter()
                .find(|entry| state.tag_of(entry).eq_ignore_ascii_case(tag))
                .ok_or(format!("no object is tagged {tag}"))?;
            let target = Target::Object(found.kind, found.index);
            if !state.editor.selection().contains(&target) {
                state.editor.select(Some(target), true);
            }
        }
        ["select", tag] => {
            let tags: Vec<(Entry, String)> = state
                .editor
                .entries()
                .into_iter()
                .map(|entry| {
                    let tag = state.tag_of(&entry);
                    (entry, tag)
                })
                .collect();
            let found = tags
                .iter()
                .find(|(_, own)| own.eq_ignore_ascii_case(tag))
                .ok_or(format!("no object is tagged {tag}"))?;
            state
                .editor
                .select(Some(Target::Object(found.0.kind, found.0.index)), false);
        }
        ["copy"] => {
            state.copied = Some(copy_text(state)?);
        }
        ["copy", "to", path @ ..] => {
            let path = path.join(" ");
            std::fs::write(&path, copy_text(state)?).map_err(|error| format!("{path}: {error}"))?;
            state.say(format!("copied to {path}"));
        }
        ["paste", "from", path @ ..] => {
            let path = path.join(" ");
            let text =
                std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
            paste(state, &text, cameras)?;
        }
        ["move", "by", dx, dy, dz] => {
            let by = point(&[dx, dy, dz])?;
            let moves = state.editor.moves_by(by);
            state.apply_many(moves);
        }
        ["move", rest @ ..] => {
            let target = selected(state)?;
            state.apply(Edit::Move(target, point(rest)?));
        }
        ["nudge", rest @ ..] => {
            let target = selected(state)?;
            let by = point(rest)?;
            let (at, _) = state
                .editor
                .placement(target)
                .ok_or("the selection is gone")?;
            state.apply(Edit::Move(target, [0, 1, 2].map(|i| at[i] + by[i])));
        }
        ["turn", degrees] => {
            let target = selected(state)?;
            state.apply(Edit::Turn(target, number(degrees)?));
        }
        ["scale", scale] => match selected(state)? {
            Target::Prop(at) => state.apply(Edit::Scale(at, number(scale)?)),
            _ => return Err("only a prop has a scale".into()),
        },
        ["size", rest @ ..] => match selected(state)? {
            Target::Box(at) => state.apply(Edit::Resize(at, point(rest)?)),
            _ => return Err("only a box has a size".into()),
        },
        // Several selected: each is copied, or deleted, as one edit.
        ["duplicate"] => {
            let selection = state.editor.selection();
            if selection.len() > 1 {
                state.apply_many(selection.into_iter().map(Edit::Duplicate).collect());
            } else {
                let target = selected(state)?;
                state.apply(Edit::Duplicate(target));
            }
        }
        ["delete"] => {
            if state.editor.selection().len() > 1 {
                let deletes = state.editor.deletes();
                state.apply_many(deletes);
            } else {
                let target = selected(state)?;
                state.apply(Edit::Delete(target));
            }
        }
        ["turn", "by", degrees] => {
            let degrees: f32 = degrees
                .parse()
                .map_err(|_| format!("{degrees:?} is not a number of degrees"))?;
            let centre = state
                .editor
                .selection_centre()
                .ok_or("nothing is selected")?;
            let turns =
                state
                    .editor
                    .turns_round(centre, degrees, collision::trig::sin_cos_deg(degrees));
            state.apply_many(turns);
        }
        ["undo"] => {
            if !state.editor.undo() {
                return Err("nothing to undo".into());
            }
            state.quiet = 0;
        }
        ["redo"] => {
            if !state.editor.redo() {
                return Err("nothing to redo".into());
            }
            state.quiet = 0;
        }
        ["add", "light", shape, rest @ ..] => {
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            let light =
                light_shape(shape, at).ok_or("add light takes point, spot, panel or strip")?;
            state.apply(Edit::AddLight(light));
        }
        ["add", "lamp", model, rest @ ..] => {
            // `-` names no mesh: the whole model.
            let (mesh, rest) = match rest {
                [mesh, rest @ ..] if mesh.parse::<f32>().is_err() => (*mesh, rest),
                rest => ("-", rest),
            };
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            let mesh = if mesh == "-" { "" } else { mesh };
            add_lamp(state, level_edit::PlacedLamp::new(model, mesh, at))?;
        }
        ["select", "lamp", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.lamps.len() {
                return Err(format!("there is no lamp {at}"));
            }
            state.editor.select(Some(Target::Lamp(at)), false);
        }
        ["lamp", field, rest @ ..] => {
            let Target::Lamp(at) = selected(state)? else {
                return Err("select a lamp first".into());
            };
            let mut lamp = state.editor.project.kjk.lamps[at].clone();
            match (*field, rest) {
                ("kind", [on @ ("room" | "placeable")]) => lamp.on_placeable = *on == "placeable",
                ("reason", words) => lamp.reason = words.join(" "),
                (turn @ ("pitch" | "roll"), [degrees]) => {
                    let degrees: f32 = degrees
                        .parse()
                        .ok()
                        .filter(|degrees: &f32| degrees.is_finite())
                        .ok_or(format!("{degrees:?} is not a number of degrees"))?;
                    if turn == "pitch" {
                        lamp.pitch = degrees;
                    } else {
                        lamp.roll = degrees;
                    }
                }
                _ => {
                    return Err(
                        "lamp kind room|placeable, lamp pitch DEGREES, lamp roll DEGREES or lamp reason TEXT"
                            .into(),
                    );
                }
            }
            state.apply(Edit::SetLamp(at, lamp));
        }
        ["lamps", "list"] => {
            let rooms = state.setup.rooms.iter().map(|room| room.model.clone());
            state.lamp_library.start(&state.setup.resolver, rooms);
            let game = state.setup.game.key();
            state.lamp_library.finish(&state.setup.resolver, game);
            let lines: Vec<String> = (state.lamp_library.found.iter())
                .map(|lamp| {
                    let mesh = if lamp.mesh.is_empty() {
                        "-"
                    } else {
                        &lamp.mesh
                    };
                    format!("lamp {} {mesh} ({})", lamp.model, lamp.texture)
                })
                .collect();
            state.say(format!("{} lamps to place", lines.len()));
            for line in lines {
                state.say(line);
            }
        }
        ["add", "room", model, rest @ ..] => {
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            add_room(state, model, at)?;
        }
        ["seam"] => {
            let Target::Room(at) = selected(state)? else {
                return Err("only a room meets another's seam".into());
            };
            if !snap_to_seam(state, at, false) {
                return Err(format!(
                    "no other room's edge is within {} m of this room's",
                    level_edit::map::SEAM_REACH
                ));
            }
        }
        ["entry", rest @ ..] => {
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            state.apply(Edit::Move(Target::Entry, at));
        }
        ["add", kind, template, rest @ ..] => {
            let kind = kind_named(kind).ok_or(
                "add takes placeable, creature, door, waypoint, trigger, sound, store or encounter",
            )?;
            let position = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            add_object(state, kind, template, position)?;
        }
        ["tag", tag] => {
            let target = selected(state)?;
            state.change_template(target, Change::Tag(tag.to_string()));
        }
        ["prop", model, rest @ ..] => {
            let model = fmt_ja::game_path(model);
            if !state.setup.models.contains(&model) {
                let known = state
                    .setup
                    .ja
                    .as_ref()
                    .is_some_and(|stack| stack.base().contains_key(model.as_str()));
                if !known {
                    return Err(format!("Jedi Academy's archives have no {model}"));
                }
            }
            let (position, scale) = match rest {
                [] => (place_point(state, cameras), 1.0),
                [x, y, z] => (point(&[x, y, z])?, 1.0),
                [x, y, z, scale] => (point(&[x, y, z])?, number(scale)?),
                _ => return Err("prop takes a model, then x y z and a scale".into()),
            };
            let name = state.editor.project.kjk.free_name("prop");
            state.apply(Edit::AddProp(Prop {
                name,
                model,
                position,
                yaw: 0.0,
                scale,
                solid: true,
            }));
        }
        ["box", rest @ ..] => {
            let (centre, size, yaw) = match rest {
                [] => {
                    let mut at = place_point(state, cameras);
                    at[2] += NEW_BOX[2] * 0.5;
                    (at, NEW_BOX, 0.0)
                }
                [cx, cy, cz, sx, sy, sz] => (point(&[cx, cy, cz])?, point(&[sx, sy, sz])?, 0.0),
                [cx, cy, cz, sx, sy, sz, yaw] => {
                    (point(&[cx, cy, cz])?, point(&[sx, sy, sz])?, number(yaw)?)
                }
                _ => return Err("box takes a centre, a size and a yaw".into()),
            };
            let name = state.editor.project.kjk.free_name("box");
            state.apply(Edit::AddBox(CollisionBox {
                name,
                centre,
                size,
                yaw,
                role: BoxRole::Solid,
            }));
        }
        ["role", word] => {
            let Target::Box(at) = selected(state)? else {
                return Err("only a box has a role".into());
            };
            let role =
                BoxRole::from_word(word).ok_or("role takes solid, walkable, block or floor")?;
            state.apply(Edit::Role(at, role));
        }
        ["light", "view", view, rest @ ..] => {
            use kjk_game::fitted_lights::Mode;
            state.light_view = match *view {
                "game" => Mode::Off,
                "live" => Mode::Live,
                "lamps" => Mode::LampsOnly,
                "baked" => Mode::Baked,
                _ => return Err("light view takes game, live, lamps or baked".into()),
            };
            if let [folder] = rest {
                state.bake = Some(PathBuf::from(folder));
            } else if state.light_view == Mode::Baked && state.bake.is_none() {
                state.bake = state.setup.bakes.first().cloned();
                if state.bake.is_none() {
                    state.say("no bake of this area was found; name its folder");
                }
            }
            state.quiet = SETTLE_FRAMES;
        }
        ["lights", on @ ("on" | "off")] => {
            state.show_lights = *on == "on";
            state.quiet = SETTLE_FRAMES;
        }
        ["select", "light", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.lights.len() {
                return Err(format!("there is no placed light {at}"));
            }
            state.editor.select(Some(Target::Light(at)), false);
        }
        ["select", "fitted", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.seen_lights.len() {
                return Err(format!(
                    "there is no fitted light {at} (are the lights drawn?)"
                ));
            }
            state.editor.select(Some(Target::Fitted(at)), false);
        }
        ["light", "copy"] => {
            let Target::Fitted(at) = selected(state)? else {
                return Err("only a fitted light is copied as a placed light".into());
            };
            let (mut light, source) = state.editor.seen_lights[at].clone();
            light.reason = format!("Copied from the fitted light of {source}.");
            state.apply(Edit::AddLight(light));
        }
        [
            "light",
            field @ ("colour" | "radius" | "cone" | "strip" | "reason"),
            rest @ ..,
        ] => {
            let Target::Light(at) = selected(state)? else {
                return Err("only a light placed by hand is changed".into());
            };
            let mut light = state.editor.project.kjk.lights[at].clone();
            match (*field, rest) {
                ("colour", [r, g, b]) => light.colour = [number(r)?, number(g)?, number(b)?],
                ("radius", [r]) => light.radius = number(r)?,
                ("cone", [c]) => {
                    light.cone = number(c)?;
                    if light.cone > 0.0 && light.aim == [0.0; 3] {
                        light.aim = [0.0, 0.0, -1.0];
                    }
                }
                ("strip", [half]) => {
                    light.half = number(half)?;
                    if light.half > 0.0 && light.way == [0.0; 3] {
                        light.way = [1.0, 0.0, 0.0];
                    }
                }
                ("reason", words) => light.reason = words.join(" "),
                _ => {
                    return Err(
                        "light colour R G B, radius R, cone C, strip HALF or reason TEXT".into(),
                    );
                }
            }
            state.apply(Edit::SetLight(at, light));
        }
        ["kind", field, rest @ ..] => {
            let mut kind = selected_kind(state)?;
            match (*field, rest) {
                ("strength", [s]) => kind.strength = number(s)?,
                ("reach", [r]) => kind.reach = number(r)?,
                ("colour", [r, g, b]) => kind.colour = [number(r)?, number(g)?, number(b)?],
                ("cone", [c]) => kind.cone = number(c)?,
                ("lamp", [on @ ("on" | "off")]) => kind.lamp = *on == "on",
                ("reason", words) => kind.reason = words.join(" "),
                _ => {
                    return Err(
                        "kind strength S, reach R, colour R G B, cone C, lamp on|off or reason TEXT"
                            .into(),
                    );
                }
            }
            set_kind(state, kind);
        }
        ["level", room, level] => {
            let level = match *level {
                "none" => None,
                level => Some(number(level)?),
            };
            state.apply(Edit::SetLevel(room.to_string(), level));
        }
        ["bake"] | ["audit"] if !state.setup.tools => return Err(NO_TOOLS.into()),
        ["bake"] => {
            if state.bake_job.is_some() || state.bake_request.is_some() {
                return Err("a bake is running".into());
            }
            command(state, &["save"], cameras)?;
            state.bake_request = Some(crate::bake::label_now());
            state.quiet = SETTLE_FRAMES;
        }
        ["bake", "compare"] => {
            let [newest, before, ..] = state.setup.bakes.as_slice() else {
                return Err("there are not two bakes of this area to compare".into());
            };
            let other = if state.bake.as_ref() == Some(newest) {
                before.clone()
            } else {
                newest.clone()
            };
            state.say(format!("showing the bake {}", other.display()));
            state.bake = Some(other);
            state.light_view = kjk_game::fitted_lights::Mode::Baked;
        }
        ["walk"] => {
            command(state, &["save"], cameras)?;
            walk(state, None)?;
        }
        ["walk", "here"] => {
            let spot = view_spot(state, cameras)?;
            command(state, &["save"], cameras)?;
            walk(state, Some(spot))?;
        }
        ["nav", on @ ("on" | "off")] => {
            state.show_nav = *on == "on";
            state.quiet = SETTLE_FRAMES;
        }
        ["outline", rest @ ..] => {
            let target = selected(state)?;
            let Target::Object(kind @ (Kind::Trigger | Kind::Encounter), index) = target else {
                return Err("select a trigger or an encounter".into());
            };
            let z = state
                .editor
                .entry(kind, index)
                .map_or(0.0, |entry| entry.position[2]);
            if rest.len() < 6 || rest.len() % 2 != 0 {
                return Err("an outline is three or more points, X Y each".into());
            }
            let points = rest
                .chunks(2)
                .map(|pair| Ok([number(pair[0])?, number(pair[1])?, z]))
                .collect::<Result<Vec<_>, String>>()?;
            state.apply(Edit::Outline(target, points));
        }
        ["area", rest @ ..] => area_command(state, rest)?,
        ["talk", name] => {
            let target = selected(state)?;
            let name = if *name == "-" { "" } else { name };
            state.change_template(target, Change::Conversation(name.to_ascii_lowercase()));
        }
        ["conversation", rest @ ..] => conversation_command(state, rest, cameras)?,
        ["blocks", event, "clear"] => {
            let target = selected(state)?;
            set_blocks(state, target, event, Vec::new())?;
        }
        ["blocks", event, "remove", n] => {
            let target = selected(state)?;
            let mut blocks = event_blocks(state, target, event)?
                .1
                .unwrap_or_else(Vec::new);
            let n: usize = n
                .parse()
                .map_err(|_| format!("{n:?} is not a block's place"))?;
            if n == 0 || n > blocks.len() {
                return Err(format!("there is no block {n}"));
            }
            blocks.remove(n - 1);
            set_blocks(state, target, event, blocks)?;
        }
        ["blocks", event, "add", key, values @ ..] => {
            let target = selected(state)?;
            let kind =
                level_edit::blocks::kind(key).ok_or(format!("{key:?} is not a kind of block"))?;
            let mut block = level_edit::blocks::Block::new(kind);
            for (slot, value) in block.values.iter_mut().zip(values.iter()) {
                *slot = value.to_string();
            }
            let mut blocks = event_blocks(state, target, event)?
                .1
                .unwrap_or_else(Vec::new);
            blocks.push(block);
            set_blocks(state, target, event, blocks)?;
        }
        ["link", "none"] => link(state, "", "")?,
        ["link", module] => link(state, module, "")?,
        ["link", module, tag] => link(state, module, tag)?,
        ["entrance", "drop", area] => state.apply(Edit::DropOther(area.to_string())),
        ["entrance", module, kind, index] => {
            entrance(state, module, (kind, index), "")?;
        }
        ["entrance", module, kind, index, tag] => {
            entrance(state, module, (kind, index), tag)?;
        }
        // A script's whole text, from a file: for an agent, which cannot
        // type in the viewer.
        ["script", "text", name, path @ ..] if !path.is_empty() => {
            let name = name.to_ascii_lowercase();
            let path = path.join(" ");
            let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            state.draft_of(&name)?;
            if let Some(draft) = state.drafts.get_mut(&name) {
                draft.text = text;
            }
            state.say(format!(
                "{name} has the text of {path}; a save writes and compiles it"
            ));
            state.pending_ui.push(vec!["view".into(), name]);
        }
        ["script", "new", name] => new_script(state, name, false)?,
        ["script", "new", name, "conditional"] => new_script(state, name, true)?,
        ["script", "edit", name] => {
            let name = name.to_ascii_lowercase();
            state.draft_of(&name)?;
            state.pending_ui.push(vec!["view".into(), name]);
        }
        ["script", "write"] => state.write_drafts()?,
        ["script", "compile"] => {
            state.write_drafts()?;
            compile_scripts(state);
        }
        ["script", "try", name] => try_script(state, name, None, false, cameras)?,
        ["script", "try", name, tag] => try_script(state, name, Some(tag), false, cameras)?,
        ["script", "debug", name] => try_script(state, name, None, true, cameras)?,
        ["script", "debug", name, tag] => try_script(state, name, Some(tag), true, cameras)?,
        ["script", label, script] => {
            let target = selected(state)?;
            let script = if *script == "-" { "" } else { script };
            state.change_template(
                target,
                Change::Script(label.to_string(), script.to_string()),
            );
        }
        [
            "flag",
            flag @ ("useable" | "inventory"),
            on @ ("on" | "off"),
        ] => {
            let target = selected(state)?;
            let label = if *flag == "useable" {
                "Useable"
            } else {
                "HasInventory"
            };
            state.change_template(target, Change::Flag(label.to_string(), *on == "on"));
        }
        ["item", "add", item] => {
            let target = selected(state)?;
            state.change_template(target, Change::AddItem(item.to_ascii_lowercase()));
        }
        ["item", "remove", at] => {
            let target = selected(state)?;
            let at: usize = at.parse().map_err(|_| format!("{at:?} is not a number"))?;
            state.change_template(target, Change::RemoveItem(at));
        }
        [
            "number" | "name" | "class" | "feat" | "power" | "skill" | "equip" | "spawn"
            | "spawnpoint",
            ..,
        ]
        | ["item", _, "drops" | "infinite", _] => field_command(state, words)?,
        ["navlink", kind @ ("jump" | "drop"), x1, y1, z1, x2, y2, z2] => {
            let number = |word: &str| {
                word.parse::<f32>()
                    .map_err(|_| format!("{word:?} is not a number"))
            };
            let kind = if *kind == "jump" {
                collision::nav::LinkKind::Jump
            } else {
                collision::nav::LinkKind::Drop
            };
            state.apply(Edit::AddLink(level_edit::area::NavLink {
                kind,
                from: [number(x1)?, number(y1)?, number(z1)?],
                to: [number(x2)?, number(y2)?, number(z2)?],
            }));
        }
        ["navlink", "remove", at] => {
            let at: usize = at.parse().map_err(|_| format!("{at:?} is not a number"))?;
            state.apply(Edit::RemoveLink(at));
        }
        ["collision", on @ ("on" | "off")] => {
            state.show_collision = *on == "on";
            state.quiet = SETTLE_FRAMES;
        }
        ["save"] => save(state)?,
        ["play"] => {
            command(state, &["save"], cameras)?;
            play(state, None)?;
        }
        ["play", "here"] => {
            let spot = view_spot(state, cameras)?;
            command(state, &["save"], cameras)?;
            play(state, Some(spot))?;
        }
        // The audit takes a while, so it runs on a thread of its own and
        // `redraw` shows its result when it ends.
        ["audit"] => {
            if state.audit_job.is_some() {
                return Err("an audit is running".into());
            }
            command(state, &["save"], cameras)?;
            let project = &state.editor.project;
            let mut audit = std::process::Command::new(&state.setup.program);
            audit.args([
                "--project",
                &project.name,
                "collision",
                "audit",
                &project.game,
                &project.module,
            ]);
            let result = AuditResult::default();
            let slot = result.clone();
            std::thread::spawn(move || {
                let output = audit
                    .output()
                    .map_err(|error| format!("kjk collision audit did not start: {error}"));
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(output);
                }
            });
            state.audit_job = Some((std::time::Instant::now(), result));
            state.say("the audit started");
        }
        _ => return Err("not an edit command (docs/LEVEL-EDIT.md lists them)".into()),
    }
    Ok(())
}

/// Shows the result of an audit that ended: its lines kept for the details
/// panel and the dump, and its summary said.
fn finish_audit(state: &mut EditorState, output: &std::process::Output, seconds: u64) {
    let text = String::from_utf8_lossy(&output.stdout);
    state.audit = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    // Each reach's line ends "... N regions: E escapes, P perches, ...".
    let count = |line: &str, word: &str| {
        line.split(',').find_map(|part| {
            part.trim()
                .strip_suffix(word)?
                .rsplit(' ')
                .next()?
                .parse::<usize>()
                .ok()
        })
    };
    let reaches: Vec<String> = state
        .audit
        .iter()
        .filter(|line| {
            let line = line.trim_start();
            ["walk ", "jump ", "force "]
                .iter()
                .any(|reach| line.starts_with(reach))
        })
        .cloned()
        .collect();
    let most = |word: &str| {
        reaches
            .iter()
            .filter_map(|line| count(line, word))
            .max()
            .unwrap_or(0)
    };
    let (escapes, perches) = (most(" escapes"), most(" perches"));
    if !output.status.success() && reaches.is_empty() {
        state.say(format!(
            "the audit failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return;
    }
    state.say(format!(
        "audit, {seconds} s: at the highest reach a body escapes the level in {escapes} regions and reaches {perches} perches off the walkmesh"
    ));
}

/// Starts a bake of the area from `scene`'s lights (`bake`): its dump is
/// written first, then `kjk detail-bake` runs on a thread of its own.
fn start_bake(
    state: &EditorState,
    scene: &LoadedScene,
    label: String,
) -> Result<crate::bake::BakeJob, String> {
    let project = &state.editor.project;
    let light_data = (state.setup.light_data.clone())
        .ok_or("no light data folder was found (set KJK_LIGHT_DATA)")?;
    let root = crate::checkout_of(&state.setup.program)
        .ok_or("the kjk checkout this editor was built in was not found")?;
    let folder = crate::bake::folder_of(&root, &project.module);
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let dump = folder.join(format!("lights-{label}.txt"));
    kjk_game::fitted_lights::dump_lights(scene, &dump.to_string_lossy());
    if !dump.is_file() {
        return Err(format!(
            "the lights' dump {} was not written",
            dump.display()
        ));
    }
    let entry = std::fs::read_to_string(light_data.join("bake.toml"))
        .map(|text| level_edit::levels::bake_entry(&text, &project.module))
        .map_err(|error| format!("bake.toml: {error}"))?;
    Ok(crate::bake::BakeJob::start(crate::bake::Plan {
        program: state.setup.program.clone(),
        project: project.name.clone(),
        module: project.module.clone(),
        game: project.game.clone(),
        light_data,
        entry,
        folder,
        label,
        dump,
    }))
}

/// The kind of fitting of the selected light the game places (a fitted
/// light): as the editor has changed it, else as the light data holds it,
/// else the general rules' faint light for a texture the data does not
/// name.
pub(crate) fn selected_kind(state: &EditorState) -> Result<level_edit::kinds::KindEdit, String> {
    let Target::Fitted(at) = selected(state)? else {
        return Err("only a light the game places at a fitting has a kind".into());
    };
    let (_, texture) = state
        .editor
        .seen_lights
        .get(at)
        .ok_or("the light is gone")?;
    if texture == "hidden lamp" {
        return Err("a room's hidden lamp has no kind".into());
    }
    let placeable = state
        .editor
        .seen_on_placeable
        .get(at)
        .copied()
        .unwrap_or(false);
    kind_of(state, texture, placeable)
}

/// The kind `texture` (on placeables or rooms) as the editor has it.
pub(crate) fn kind_of(
    state: &EditorState,
    texture: &str,
    placeable: bool,
) -> Result<level_edit::kinds::KindEdit, String> {
    if let Some(kind) = (state.editor.project.kinds)
        .iter()
        .find(|kind| kind.texture == texture && kind.placeable == placeable)
    {
        return Ok(kind.clone());
    }
    let folder = state
        .setup
        .light_data
        .as_ref()
        .ok_or("no light data folder was found (set KJK_LIGHT_DATA)")?;
    let path = folder.join(format!("{}.toml", state.editor.project.game));
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(
        level_edit::kinds::read_kind(&text, texture, placeable).unwrap_or(
            level_edit::kinds::KindEdit {
                texture: texture.to_string(),
                placeable,
                lamp: true,
                form: "faint".into(),
                colour: [1.0; 3],
                reach: 3.0,
                strength: 0.5,
                cone: 0.2,
                reason: String::new(),
            },
        ),
    )
}

/// Changes a kind of fitting (`Edit::SetKind`), keeping how the light data
/// held it the first time, for the view to show again on undo.
pub(crate) fn set_kind(state: &mut EditorState, kind: level_edit::kinds::KindEdit) {
    let key = (kind.texture.clone(), kind.placeable);
    if !state.kind_originals.contains_key(&key)
        && let Ok(original) = kind_of(state, &kind.texture, kind.placeable)
    {
        state.kind_originals.insert(key, original);
    }
    state.apply(Edit::SetKind(kind));
}

/// A light placed by hand of a ready shape at `at`: a lamp that shines all
/// ways, a spotlight straight down, a panel's light that shines along +X,
/// or a strip 1 m long each way along X.
pub(crate) fn light_shape(shape: &str, at: [f32; 3]) -> Option<level_edit::PlacedLight> {
    let base = level_edit::PlacedLight {
        at,
        ..level_edit::PlacedLight::default()
    };
    Some(match shape {
        "point" => base,
        "spot" => level_edit::PlacedLight {
            aim: [0.0, 0.0, -1.0],
            cone: 0.7,
            ..base
        },
        "panel" => level_edit::PlacedLight {
            aim: [1.0, 0.0, 0.0],
            cone: 0.2,
            radius: 3.0,
            ..base
        },
        "strip" => level_edit::PlacedLight {
            way: [1.0, 0.0, 0.0],
            half: 1.0,
            ..base
        },
        _ => return None,
    })
}

/// Adds `lamp`, refused when the install has no such model or the model
/// has no such mesh.
pub(crate) fn add_lamp(
    state: &mut EditorState,
    lamp: level_edit::PlacedLamp,
) -> Result<(), String> {
    kjk_game::lamps::model(&state.setup.resolver, &lamp)?;
    state.apply(Edit::AddLamp(lamp));
    Ok(())
}

/// Where the view looks down to the level, and the way the camera faces
/// (degrees from +X): a place for the player to start (`kjk play --at`).
pub(crate) fn view_spot(
    state: &mut EditorState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<([f32; 3], f32), String> {
    let ray = view_centre(cameras)
        .and_then(|centre| ray_at(cameras, centre))
        .ok_or("the view has no camera")?;
    let at =
        ground(state, ray).ok_or("the view's centre does not meet the level: look at a floor")?;
    let ahead = kjk_game::scene::game_to_bevy().inverse() * ray.1;
    let facing = ahead.y.atan2(ahead.x).to_degrees();
    Ok((at, facing))
}

/// The `--at` of a place to start.
fn at_arg((at, facing): ([f32; 3], f32)) -> String {
    format!("{},{},{},{facing:.0}", at[0], at[1], at[2])
}

/// Starts `kjk play` on the project's module, at its entry or at `spot`.
fn play(state: &mut EditorState, spot: Option<([f32; 3], f32)>) -> Result<(), String> {
    let project = state.editor.project.clone();
    let mut play = std::process::Command::new(&state.setup.program);
    play.args([
        "--project",
        &project.name,
        "play",
        "--stage",
        &project.module,
        "--class",
        "jedi",
    ]);
    if let Some(spot) = spot {
        play.args(["--at", &at_arg(spot)]);
    }
    play.spawn()
        .map_err(|error| format!("kjk play did not start: {error}"))?;
    state.say(match spot {
        Some(spot) => format!("kjk play started on {} at {}", project.module, at_arg(spot)),
        None => format!("kjk play started on {}", project.module),
    });
    Ok(())
}

/// Starts play's walk mode (`kjk play --walk`) on the project, lit as the
/// view is: its mode, its bake, and the light data the project saves to.
pub(crate) fn walk(state: &mut EditorState, spot: Option<([f32; 3], f32)>) -> Result<(), String> {
    use kjk_game::fitted_lights::Mode;
    let project = state.editor.project.clone();
    let mut walk = std::process::Command::new(&state.setup.program);
    walk.args([
        "--project",
        &project.name,
        "play",
        "--stage",
        &project.module,
        "--walk",
    ]);
    if let Some(spot) = spot {
        walk.args(["--at", &at_arg(spot)]);
    }
    walk.env(
        "KJK_FITTED_LIGHTS",
        match state.light_view {
            Mode::Off => "off",
            Mode::Live => "on",
            Mode::LampsOnly => "lamps",
            Mode::Baked => "baked",
        },
    );
    if let (Mode::Baked, Some(bake)) = (state.light_view, &state.bake) {
        walk.env("KJK_DETAIL_BAKE", bake);
    }
    if let Some(folder) = &state.setup.light_data {
        walk.env("KJK_LIGHT_DATA", folder);
    }
    walk.spawn()
        .map_err(|error| format!("kjk play --walk did not start: {error}"))?;
    state.say(format!("walk mode started on {}", project.module));
    Ok(())
}

/// `edit number|name|class|feat|power|skill|equip|item|spawn|spawnpoint
/// ...`: a creature's, a store's or an encounter's fields
/// (`level_edit::fields`), on the selected object.
fn field_command(state: &mut EditorState, words: &[&str]) -> Result<(), String> {
    use level_edit::fields::{FieldChange, Spawn};
    let target = selected(state)?;
    let number = |word: &str| {
        word.parse::<f64>()
            .map_err(|_| format!("{word:?} is not a number"))
    };
    let whole = |word: &str| {
        word.parse::<u16>()
            .map_err(|_| format!("{word:?} is not a whole number"))
    };
    let place = |word: &str| {
        word.parse::<usize>()
            .map_err(|_| format!("{word:?} is not a place in a list"))
    };
    let on = |word: &str| match word {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{word:?} is not on or off")),
    };
    let change = match words {
        ["number", label, value] => FieldChange::Number(label.to_string(), number(value)?),
        ["name", label, text @ ..] => FieldChange::Name(label.to_string(), text.join(" ")),
        ["class", at, "-"] => FieldChange::Class(place(at)?, None),
        ["class", at, row, levels] => {
            FieldChange::Class(place(at)?, Some((whole(row)?, whole(levels)?)))
        }
        ["feat", row, switch] => FieldChange::Feat(whole(row)?, on(switch)?),
        ["power", class, row, switch] => {
            FieldChange::Power(place(class)?, whole(row)?, on(switch)?)
        }
        ["skill", at, rank] => FieldChange::Skill(
            place(at)?,
            rank.parse::<u8>()
                .map_err(|_| format!("{rank:?} is not a rank"))?,
        ),
        ["equip", slot, item] => {
            let slot = level_edit::fields::SLOTS
                .iter()
                .find(|(number, name)| {
                    number.to_string() == *slot
                        || name.eq_ignore_ascii_case(&slot.replace('_', " "))
                })
                .map(|(number, _)| *number)
                .ok_or(format!("{slot:?} is not an equipment slot"))?;
            let item = if *item == "-" { "" } else { item };
            FieldChange::Equip(slot, item.to_ascii_lowercase())
        }
        ["item", at, flag, switch] => FieldChange::ItemFlag(
            place(at)?,
            if *flag == "drops" {
                "Dropable"
            } else {
                "Infinite"
            },
            on(switch)?,
        ),
        ["spawn", at, "-"] => FieldChange::Spawn(place(at)?, None),
        ["spawn", at, template, cr, rest @ ..] => FieldChange::Spawn(
            place(at)?,
            Some(Spawn {
                template: template.to_ascii_lowercase(),
                cr: number(cr)? as f32,
                single: rest.first() == Some(&"single"),
            }),
        ),
        ["spawnpoint", "add", x, y, z, facing @ ..] => {
            let Target::Object(Kind::Encounter, index) = target else {
                return Err("only an encounter has spawn points".into());
            };
            let facing = facing.first().map_or(Ok(0.0), |word| number(word))? as f32;
            let mut points = level_edit::git::spawn_points(&state.editor.project.git, index);
            points.push((
                [number(x)? as f32, number(y)? as f32, number(z)? as f32],
                facing,
            ));
            state.apply(Edit::SpawnPoints(target, points));
            return Ok(());
        }
        ["spawnpoint", "remove", at] => {
            let Target::Object(Kind::Encounter, index) = target else {
                return Err("only an encounter has spawn points".into());
            };
            let mut points = level_edit::git::spawn_points(&state.editor.project.git, index);
            let at = place(at)?;
            if at >= points.len() {
                return Err(format!("there is no spawn point {at}"));
            }
            points.remove(at);
            state.apply(Edit::SpawnPoints(target, points));
            return Ok(());
        }
        _ => return Err("that is not a change of a template's fields".into()),
    };
    state.change_template(target, Change::Field(change));
    Ok(())
}

/// What is selected, as a copy's text (`level_edit::clip`).
pub(crate) fn copy_text(state: &mut EditorState) -> Result<String, String> {
    let selection = state.editor.selection();
    let centre = state
        .editor
        .selection_centre()
        .ok_or("nothing is selected")?;
    let mut clip = level_edit::clip::Clip::of(&state.editor.project, &selection, centre);
    // The game's templates the objects use go with them too: a module's own
    // (Belaya's in the enclave's archive) is read nowhere else. A paste
    // leaves out each one the level it goes to reads the same (`paste`).
    for (kind, item) in clip.entries.clone() {
        let Some(restype) = level_edit::template::restype(kind) else {
            continue;
        };
        let label = if kind == Kind::Store {
            "ResRef"
        } else {
            "TemplateResRef"
        };
        let Some(name) = item.resref(label).map(|name| name.to_ascii_lowercase()) else {
            continue;
        };
        if clip.templates.iter().any(|(had, _)| *had == name) {
            continue;
        }
        if let Some(gff) = state
            .setup
            .resolver
            .read(&name, restype)
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        {
            clip.templates.push((
                name.clone(),
                level_edit::Template {
                    restype,
                    from: name,
                    gff,
                },
            ));
        }
    }
    if clip.is_empty() {
        return Err(
            "nothing selected is copied: only objects, props, boxes, lamps and lights are".into(),
        );
    }
    let count = clip.entries.len()
        + clip.kjk.props.len()
        + clip.kjk.boxes.len()
        + clip.kjk.lamps.len()
        + clip.kjk.lights.len();
    state.say(format!(
        "copied {count} things and the {} templates they use",
        clip.templates.len()
    ));
    Ok(clip.write())
}

/// Pastes the copy `text` with its middle where the view looks
/// (`place_point`), as one edit.
pub(crate) fn paste(
    state: &mut EditorState,
    text: &str,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    let mut clip = level_edit::clip::Clip::read(text)?;
    let at = place_point(state, cameras);
    // A copied template this level reads the same already stays out of
    // the project: the game's own shared ones, most often.
    let project = &state.editor.project;
    let resolver = &state.setup.resolver;
    clip.templates.retain(|(name, made)| {
        project.templates.contains_key(name)
            || resolver
                .read(name, made.restype)
                .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                .is_none_or(|had| had != made.gff)
    });
    // The names a pasted template must keep clear of: the install's, of
    // each copied template's kind, among the names a new one could take.
    let resolver = &state.setup.resolver;
    let mut taken = Vec::new();
    for (name, made) in &clip.templates {
        // `template::fresh_name`'s names, `kjeN_from` cut to a resref.
        let names = std::iter::once(name.clone()).chain((1..64).map(|n| {
            let mut candidate = format!("{}{n}_{}", level_edit::template::PREFIX, made.from);
            candidate.truncate(level_edit::git::RESREF_LEN);
            candidate
        }));
        for candidate in names {
            if !state.editor.project.templates.contains_key(&candidate)
                && resolver.find(&candidate, made.restype).is_some()
            {
                taken.push(candidate);
            }
        }
    }
    let count = clip.entries.len()
        + clip.kjk.props.len()
        + clip.kjk.boxes.len()
        + clip.kjk.lamps.len()
        + clip.kjk.lights.len();
    state.apply(Edit::Paste {
        clip: Box::new(clip),
        at,
        taken,
    });
    state.say(format!("pasted {count} things"));
    Ok(())
}

/// A conversation's line by its words: `line N` or `reply N`.
fn node_at(side: &str, n: &str) -> Result<(level_edit::dialog::Side, usize), String> {
    use level_edit::dialog::Side;
    let side = match side {
        "line" | "entry" => Side::Entry,
        "reply" => Side::Reply,
        _ => return Err(format!("{side:?}: a node is a line or a reply")),
    };
    let n = n
        .parse::<usize>()
        .map_err(|_| format!("{n:?} is not a node's number"))?;
    Ok((side, n))
}

/// A link by its words: `start I`, or `line N I` / `reply N I`.
fn link_at(words: &[&str]) -> Result<(level_edit::dialog::LinkAt, usize), String> {
    use level_edit::dialog::LinkAt;
    let number = |word: &str| {
        word.parse::<usize>()
            .map_err(|_| format!("{word:?} is not a link's number"))
    };
    match words {
        ["start", i, ..] => Ok((LinkAt::Start(number(i)?), 2)),
        [side, n, i, ..] => {
            let (side, n) = node_at(side, n)?;
            Ok((LinkAt::From(side, n, number(i)?), 3))
        }
        _ => Err("a link is start I, or line N I, or reply N I".into()),
    }
}

/// `edit conversation ...`: the project's conversations made, copied from
/// the game's, changed and tried (`level_edit::dialog`).
fn conversation_command(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    use level_edit::dialog::Edit as Talk;
    let none = |word: &str| {
        if word == "-" {
            String::new()
        } else {
            word.to_ascii_lowercase()
        }
    };
    match words {
        ["new", name] => {
            state.apply(Edit::NewConversation {
                name: name.to_string(),
                base: None,
            });
            state
                .pending_ui
                .push(vec!["conversation".into(), name.to_ascii_lowercase()]);
            return Ok(());
        }
        ["copy", name] => {
            let name = name.to_ascii_lowercase();
            let bytes = state
                .setup
                .resolver
                .read(&name, ResType::DLG)
                .ok_or(format!("the game has no conversation {name}"))?;
            let gff =
                fmt_odyssey::gff::Gff::parse(&bytes).map_err(|error| format!("{name}: {error}"))?;
            state.apply(Edit::NewConversation {
                name: name.clone(),
                base: Some(gff),
            });
            state.pending_ui.push(vec!["conversation".into(), name]);
            return Ok(());
        }
        ["drop", name] => {
            state.apply(Edit::DropConversation(name.to_ascii_lowercase()));
            return Ok(());
        }
        ["try", name, on, picks @ ..] => {
            let picks = picks
                .iter()
                .map(|pick| {
                    pick.parse::<u8>()
                        .map_err(|_| format!("{pick:?} is not a reply's place"))
                })
                .collect::<Result<Vec<u8>, String>>()?;
            return try_conversation(state, name, on, picks, cameras);
        }
        _ => {}
    }
    let [name, rest @ ..] = words else {
        return Err(
            "edit conversation takes new, copy, drop, try, or a conversation's name".into(),
        );
    };
    let name = name.to_ascii_lowercase();
    let change = match rest {
        ["add"] => Talk::Add { under: None },
        ["add", side, n] => Talk::Add {
            under: Some(node_at(side, n)?),
        },
        ["text", side, n, text @ ..] => {
            let (side, n) = node_at(side, n)?;
            Talk::Text(side, n, text.join(" "))
        }
        ["speaker", side, n, tag] => {
            let (side, n) = node_at(side, n)?;
            Talk::Speaker(
                side,
                n,
                if *tag == "-" {
                    String::new()
                } else {
                    tag.to_string()
                },
            )
        }
        ["listener", side, n, tag] => {
            let (side, n) = node_at(side, n)?;
            Talk::Listener(
                side,
                n,
                if *tag == "-" {
                    String::new()
                } else {
                    tag.to_string()
                },
            )
        }
        ["script", side, n, script] => {
            let (side, n) = node_at(side, n)?;
            Talk::Script(side, n, none(script))
        }
        ["quest", side, n, quest, entry] => {
            let (side, n) = node_at(side, n)?;
            let entry = entry
                .parse::<u32>()
                .map_err(|_| format!("{entry:?} is not an entry's number"))?;
            Talk::Quest(side, n, quest.to_string(), entry)
        }
        ["condition", link @ ..] => {
            let (at, used) = link_at(link)?;
            let script = link
                .get(used)
                .ok_or("a condition names a script, or - for none")?;
            Talk::Condition(at, none(script))
        }
        ["link", "start", to] => Talk::LinkTo {
            from: None,
            to: to
                .parse()
                .map_err(|_| format!("{to:?} is not a line's number"))?,
        },
        ["link", side, n, to] => Talk::LinkTo {
            from: Some(node_at(side, n)?),
            to: to
                .parse()
                .map_err(|_| format!("{to:?} is not a node's number"))?,
        },
        ["unlink", link @ ..] => Talk::Unlink(link_at(link)?.0),
        ["up", link @ ..] => Talk::Shift(link_at(link)?.0, -1),
        ["down", link @ ..] => Talk::Shift(link_at(link)?.0, 1),
        ["delete", side, n] => {
            let (side, n) = node_at(side, n)?;
            Talk::Delete(side, n)
        }
        ["end", script] => Talk::End(none(script)),
        ["abort", script] => Talk::Abort(none(script)),
        _ => {
            return Err(
                "a conversation's change is add, text, speaker, listener, script, quest, condition, link, unlink, up, down, delete, end or abort"
                    .into(),
            );
        }
    };
    state.apply(Edit::Conversation(name, change));
    Ok(())
}

/// Saves, then plays the project's conversation `name` headless with the
/// player talking to the object tagged `on`, answering by `picks`
/// (`kjk try-conversation`).
pub(crate) fn try_conversation(
    state: &mut EditorState,
    name: &str,
    on: &str,
    picks: Vec<u8>,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    if state.script_try.is_some() {
        return Err("a try is running".into());
    }
    let name = name.to_ascii_lowercase();
    command(state, &["save"], cameras)?;
    let project = &state.editor.project;
    let plan = crate::script_edit::Plan {
        program: state.setup.program.clone(),
        project: project.name.clone(),
        game: project.game.clone(),
        module: project.module.clone(),
        script: name.clone(),
        on: Some(on.to_string()),
        seconds: 40.0,
        debug: None,
        conversation: Some(picks),
    };
    state.script_try = Some(crate::script_edit::Try::start(plan));
    state.say(format!("trying the conversation {name} with {on}"));
    Ok(())
}

/// The text of the project's script `name`: its draft, else its file.
pub(crate) fn script_text(state: &EditorState, name: &str) -> Option<String> {
    if let Some(draft) = state.drafts.get(name) {
        return Some(draft.text.clone());
    }
    let bytes = std::fs::read(state.editor.project.folder.join(format!("{name}.nss"))).ok()?;
    Some(bytes.into_iter().map(char::from).collect())
}

/// The script an object runs on `event`, and its blocks: `None` when the
/// script is no script of blocks (written by hand, or the game's), and
/// none at all when the event runs nothing.
pub(crate) fn event_blocks(
    state: &EditorState,
    target: Target,
    event: &str,
) -> Result<(String, Option<Vec<level_edit::blocks::Block>>), String> {
    let Target::Object(kind, index) = target else {
        return Err("only an object of the area runs scripts".into());
    };
    let entry = state
        .editor
        .entry(kind, index)
        .ok_or("the object is not there")?;
    let gff = state
        .behaviour_source(kind, &entry)
        .ok_or("the object's template does not read")?;
    let script = level_edit::template::scripts(&gff)
        .into_iter()
        .find(|(label, _)| label == event)
        .map(|(_, script)| script)
        .ok_or(format!("the object has no event {event}"))?;
    if script.is_empty() {
        return Ok((script, Some(Vec::new())));
    }
    let blocks = script_text(state, &script).and_then(|text| level_edit::blocks::read(&text));
    Ok((script, blocks))
}

/// Gives an object's `event` the script `blocks` make: the script of blocks
/// it runs already, else a new one (`kjb_N`), written as a draft that the
/// next save writes and compiles.
pub(crate) fn set_blocks(
    state: &mut EditorState,
    target: Target,
    event: &str,
    blocks: Vec<level_edit::blocks::Block>,
) -> Result<(), String> {
    let (script, held) = event_blocks(state, target, event)?;
    let name = if !script.is_empty() && held.is_some() {
        script.clone()
    } else {
        let taken = |name: &str| {
            state.project_scripts.contains(name)
                || state.drafts.contains_key(name)
                || state.setup.scripts.binary_search(&name.to_string()).is_ok()
        };
        level_edit::blocks::fresh_name(taken)
    };
    let text = level_edit::blocks::write(&blocks);
    match state.drafts.get_mut(&name) {
        Some(draft) => draft.text = text,
        None => {
            let saved = script_text(state, &name).unwrap_or_else(String::new);
            state
                .drafts
                .insert(name.clone(), crate::script_edit::Draft { text, saved });
        }
    }
    if script != name {
        state.change_template(target, Change::Script(event.to_string(), name));
    }
    Ok(())
}

/// The area's ARE as the project has it now: a new map's, the project's
/// copy of an installed area's, else the install's.
pub(crate) fn area_are(state: &EditorState) -> Option<fmt_odyssey::gff::Gff> {
    let project = &state.editor.project;
    if let Some(map) = &project.map {
        return Some(map.are.clone());
    }
    if let Some(are) = &project.are {
        return Some(are.clone());
    }
    install_are(state)
}

/// The installed area's ARE as the install has it.
fn install_are(state: &EditorState) -> Option<fmt_odyssey::gff::Gff> {
    let bytes = state
        .setup
        .resolver
        .read(&state.editor.project.area, ResType::ARE)?;
    fmt_odyssey::gff::Gff::parse(&bytes).ok()
}

/// Changes one of the area's settings (`level_edit::settings`).
pub(crate) fn change_area(state: &mut EditorState, change: level_edit::settings::AreaChange) {
    let base = (state.editor.project.map.is_none() && state.editor.project.are.is_none())
        .then(|| install_are(state))
        .flatten();
    state.apply(Edit::Area { change, base });
}

/// `edit area ...`: the area's music, ambient sound, fog, ambient light and
/// scripts, and a new map's module scripts.
fn area_command(state: &mut EditorState, words: &[&str]) -> Result<(), String> {
    use fmt_odyssey::gff::GffValue;
    use level_edit::settings::{AreaChange, colour_value};
    let row = |word: &str| -> Result<u32, String> {
        word.parse::<u32>()
            .map_err(|_| format!("{word:?} is not a row's number"))
    };
    let colour = |words: &[&str]| -> Result<u32, String> {
        match words {
            [r, g, b] => Ok(colour_value([number(r)?, number(g)?, number(b)?])),
            _ => Err("a colour is three numbers from 0 to 1: R G B".into()),
        }
    };
    let script = |word: &str| {
        if word == "-" {
            String::new()
        } else {
            word.to_ascii_lowercase()
        }
    };
    let change = match words {
        ["music", when @ ("day" | "night" | "battle"), n] => {
            let label = match *when {
                "day" => "MusicDay",
                "night" => "MusicNight",
                _ => "MusicBattle",
            };
            AreaChange::Property(label.into(), row(n)?)
        }
        ["ambience", when @ ("day" | "night"), n] => {
            let label = if *when == "day" {
                "AmbientSndDay"
            } else {
                "AmbientSndNight"
            };
            AreaChange::Property(label.into(), row(n)?)
        }
        ["volume", when @ ("day" | "night"), n] => {
            let label = if *when == "day" {
                "AmbientSndDayVol"
            } else {
                "AmbientSndNitVol"
            };
            AreaChange::Property(label.into(), row(n)?.min(127))
        }
        ["fog", on @ ("on" | "off")] => {
            AreaChange::Are("SunFogOn".into(), GffValue::Byte(u8::from(*on == "on")))
        }
        // Both ends of the fog as one edit, one undo step.
        ["fog", near, far] => {
            let (near, far) = (number(near)?, number(far)?);
            let base = (state.editor.project.map.is_none() && state.editor.project.are.is_none())
                .then(|| install_are(state))
                .flatten();
            let edits = [("SunFogNear", near), ("SunFogFar", far)]
                .into_iter()
                .map(|(label, value)| Edit::Area {
                    change: AreaChange::Are(label.into(), GffValue::Float(value.to_bits())),
                    base: base.clone(),
                })
                .collect();
            state.apply_many(edits);
            return Ok(());
        }
        ["fog", "colour", rgb @ ..] => {
            AreaChange::Are("SunFogColor".into(), GffValue::Dword(colour(rgb)?))
        }
        ["ambient", rgb @ ..] => {
            AreaChange::Are("DynAmbientColor".into(), GffValue::Dword(colour(rgb)?))
        }
        ["script", label, name] => AreaChange::Are(
            (*label).to_string(),
            GffValue::ResRef(script(name).into_bytes()),
        ),
        ["module", "script", label, name] => {
            AreaChange::ModuleScript((*label).to_string(), script(name))
        }
        _ => {
            return Err(
                "edit area takes music, ambience, volume, fog, ambient, script or module script"
                    .into(),
            );
        }
    };
    change_area(state, change);
    Ok(())
}

/// The entry area of `module` and its GIT as the install has it: the
/// project's own area's as the editor has it, another module's read once.
pub(crate) fn module_area(
    state: &mut EditorState,
    module: &str,
) -> Result<(String, fmt_odyssey::gff::Gff), String> {
    let module = module.to_ascii_lowercase();
    if module == state.editor.project.module {
        return Ok((
            state.editor.project.area.clone(),
            state.editor.project.git.clone(),
        ));
    }
    if !state.setup.modules.contains(&module) {
        return Err(format!("the install has no module {module}"));
    }
    let game = state.setup.game;
    state
        .module_facts
        .entry(module.clone())
        .or_insert_with(|| {
            let resolver = kjk_game::viewer::resolver(&kjk_game::discover(), game, Some(&module))?;
            let ifo = resolver
                .read("module", ResType::IFO)
                .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                .ok_or(format!("{module} has no module.ifo"))?;
            let area = ifo
                .root
                .resref("Mod_Entry_Area")
                .map(|area| area.to_ascii_lowercase())
                .ok_or(format!("{module}'s module.ifo names no entry area"))?;
            let git = resolver
                .read(&area, ResType::GIT)
                .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                .ok_or(format!("{module}'s area {area} has no GIT"))?;
            Ok((area, git))
        })
        .clone()
}

/// The doors and waypoints of `module`'s entry area a link can arrive at.
pub(crate) fn arrivals(
    state: &mut EditorState,
    module: &str,
) -> Result<Vec<(String, bool)>, String> {
    module_area(state, module).map(|(_, git)| level_edit::git::arrivals(&git))
}

/// Leads the selected door or trigger to `module` (nowhere when empty),
/// arriving at the door or waypoint tagged `tag` (the module's entry when
/// empty).
pub(crate) fn link(state: &mut EditorState, module: &str, tag: &str) -> Result<(), String> {
    let target = selected(state)?;
    let Target::Object(kind @ (Kind::Door | Kind::Trigger), _) = target else {
        return Err("select a door or a trigger to lead it somewhere".into());
    };
    let _ = kind;
    let module = module.to_ascii_lowercase();
    let to_door = if module.is_empty() || tag.is_empty() {
        false
    } else {
        let arrivals = arrivals(state, &module)?;
        let found = arrivals
            .iter()
            .find(|(own, _)| own.eq_ignore_ascii_case(tag))
            .ok_or(format!("{module} has no door or waypoint tagged {tag}"))?;
        found.1
    };
    state.change_template(
        target,
        Change::Link(level_edit::template::Link {
            module,
            to: tag.to_string(),
            to_door,
        }),
    );
    Ok(())
}

/// Leads a door or trigger of `module`'s entry area (its kind and its place
/// in that kind's list) into this project's level, arriving at the door or
/// waypoint tagged `tag` here (the entry when empty).
pub(crate) fn entrance(
    state: &mut EditorState,
    module: &str,
    (kind, index): (&str, &str),
    tag: &str,
) -> Result<(), String> {
    let module = module.to_ascii_lowercase();
    if module == state.editor.project.module {
        return Err("that is this level: select the door or trigger and set where it leads".into());
    }
    let kind = match kind {
        "door" => Kind::Door,
        "trigger" => Kind::Trigger,
        _ => return Err(format!("{kind:?}: a way in is a door or a trigger")),
    };
    let index: usize = index
        .parse()
        .map_err(|_| format!("{index:?} is not a place in the list"))?;
    let (area, git) = module_area(state, &module)?;
    let own = state.editor.project.module.clone();
    let to_door = if tag.is_empty() {
        false
    } else {
        let here = level_edit::git::arrivals(&state.editor.project.git);
        here.iter()
            .find(|(own_tag, _)| own_tag.eq_ignore_ascii_case(tag))
            .ok_or(format!("this level has no door or waypoint tagged {tag}"))?
            .1
    };
    let base = (!state.editor.project.others.contains_key(&area)).then_some(git);
    state.apply(Edit::LinkOther {
        module,
        area,
        base,
        kind,
        index,
        link: level_edit::template::Link {
            module: own,
            to: tag.to_string(),
            to_door,
        },
    });
    Ok(())
}

/// A new script of the project, `name`, opened in the viewer: a `main`, or
/// a conversation's `StartingConditional`.
fn new_script(state: &mut EditorState, name: &str, conditional: bool) -> Result<(), String> {
    let name = name.to_ascii_lowercase();
    if state.project_scripts.contains(&name) || state.drafts.contains_key(&name) {
        return Err(format!("the project has a script {name} already"));
    }
    if let Some(problem) = crate::script_edit::name_problem(&name) {
        return Err(problem);
    }
    state.drafts.insert(
        name.clone(),
        crate::script_edit::Draft {
            text: crate::script_edit::template(&name, conditional),
            saved: String::new(),
        },
    );
    state.say(format!("new script {name}"));
    state.pending_ui.push(vec!["view".into(), name]);
    Ok(())
}

/// Saves (which writes and compiles the scripts), then tries `name` for the
/// object tagged `on` (else the player) in the module installed headless.
fn try_script(
    state: &mut EditorState,
    name: &str,
    on: Option<&str>,
    debug: bool,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    if state.script_try.is_some() {
        return Err("a script is being tried".into());
    }
    let name = name.to_ascii_lowercase();
    command(state, &["save"], cameras)?;
    if let Some(why) = state.script_errors.get(&name) {
        return Err(format!("{name} did not compile: {why}"));
    }
    let project = &state.editor.project;
    let plan = crate::script_edit::Plan {
        program: state.setup.program.clone(),
        project: project.name.clone(),
        game: project.game.clone(),
        module: project.module.clone(),
        script: name.clone(),
        on: on.map(str::to_string),
        seconds: 3.0,
        debug: debug.then(|| project.folder.join(format!("{name}.ndb"))),
        conversation: None,
    };
    state.script_try = Some(crate::script_edit::Try::start(plan));
    state.say(format!("trying {name} for {}", on.unwrap_or("the player")));
    Ok(())
}

/// Compiles the project's scripts (`level_edit::scripts`) and says how it
/// went: each mistake, with its file and line.
fn compile_scripts(state: &mut EditorState) {
    let folder = state.editor.project.folder.clone();
    if level_edit::scripts::sources(&folder).is_empty() {
        return;
    }
    let Some(catalog) = state.setup.catalog.clone() else {
        state.say("the project's scripts were not compiled: the install has no nwscript.nss");
        return;
    };
    let resolver = state.setup.resolver.clone();
    let install = |name: &str| resolver.read(name, ResType::NSS);
    let compiled = level_edit::scripts::compile_all(&folder, &install, &catalog);
    state.script_errors.clear();
    state.script_round = state.script_round.wrapping_add(1);
    let good = compiled.iter().filter(|each| each.outcome.is_ok()).count();
    for each in compiled {
        if let Err(why) = each.outcome {
            state.say(format!("{} did not compile: {why}", each.name));
            state.script_errors.insert(each.name, why);
        }
    }
    if good > 0 {
        state.say(format!("compiled {good} of the project's scripts"));
    }
}

/// Saves the project (`Editor::save_with`), saying its warnings and what
/// was written, then the light data's part (`save_light_data`). When the
/// light data does not save, the project's own files are saved all the
/// same, and the error says so.
pub(crate) fn save(state: &mut EditorState) -> Result<(), String> {
    let made_by = format!("kjk {}", kjk_game::GIT_SHA);
    let warnings: Vec<String> = state
        .editor
        .project
        .problems()
        .into_iter()
        .filter(|problem| !problem.blocks)
        .map(|problem| problem.what)
        .collect();
    for warning in warnings {
        state.say(warning);
    }
    let resolver = state.setup.resolver.clone();
    let read = |name: &str, restype: ResType| resolver.read(name, restype);
    let written = state.editor.save_with(&made_by, &read)?;
    state.write_drafts()?;
    crate::recover::clear(state);
    state.say(format!(
        "saved {} files to {}",
        written.len(),
        state.editor.project.folder.display()
    ));
    compile_scripts(state);
    save_light_data(state).map_err(|why| {
        format!("the project's own files were saved, but the light data was not: {why}")
    })
}

/// Saves what the light data holds of the level: the kinds of fitting
/// changed, the levels the bake's rooms aim at, and an installed level's
/// lights and lamps placed by hand.
fn save_light_data(state: &mut EditorState) -> Result<(), String> {
    // Without a light data folder, an installed level's lights and lamps
    // placed by hand have nowhere to go: said, never dropped in silence.
    let project = &state.editor.project;
    let (lights, lamps) = (project.kjk.lights.len(), project.kjk.lamps.len());
    if project.map.is_none() && state.setup.light_data.is_none() && lights + lamps > 0 {
        let count = |n: usize, one: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {one}s")
            }
        };
        return Err(format!(
            "{} and {} placed by hand were not saved, because {NO_LIGHT_DATA}",
            count(lights, "light"),
            count(lamps, "lamp")
        ));
    }
    // The kinds of fitting changed go to the light data: their `[[kind]]`
    // entries, which the game reads, and a `[[fitting]]` each, which
    // `kinds.py` reads.
    let changed: Vec<level_edit::kinds::KindEdit> = (state.editor.project.kinds)
        .iter()
        .filter(|kind| {
            state
                .kind_originals
                .get(&(kind.texture.clone(), kind.placeable))
                .is_none_or(|original| original != *kind)
        })
        .cloned()
        .collect();
    if !changed.is_empty() {
        let folder = (state.setup.light_data.clone())
            .ok_or("no light data folder was found (set KJK_LIGHT_DATA)")?;
        let game = state.editor.project.game.clone();
        let kinds_path = folder.join(format!("{game}.toml"));
        let fittings_path = folder.join("fittings.toml");
        let read = |path: &std::path::Path| {
            std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
        };
        let (mut kinds_text, mut fittings_text) = (read(&kinds_path)?, read(&fittings_path)?);
        for kind in &changed {
            kinds_text = level_edit::kinds::write_kind(&kinds_text, kind)?;
            fittings_text = level_edit::kinds::write_fitting(&fittings_text, &game, kind)?;
        }
        for (path, text) in [(&kinds_path, kinds_text), (&fittings_path, fittings_text)] {
            std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        for kind in &changed {
            state
                .kind_originals
                .insert((kind.texture.clone(), kind.placeable), kind.clone());
        }
        let message = format!(
            "saved {} kinds of fitting to {} and {}",
            changed.len(),
            kinds_path.display(),
            fittings_path.display()
        );
        state.say(message);
    }
    // The levels the bake's rooms aim at go to the module's levels file,
    // which `bake.toml` names (and is made to name, the first time).
    if state.editor.project.levels != state.levels_original {
        let folder = (state.setup.light_data.clone())
            .ok_or("no light data folder was found (set KJK_LIGHT_DATA)")?;
        let module = state.editor.project.module.clone();
        let file =
            (state.bake_entry.levels.clone()).unwrap_or_else(|| format!("levels/{module}.txt"));
        let path = folder.join(&file);
        let old = std::fs::read_to_string(&path).ok();
        let text = level_edit::levels::write_levels(
            old.as_deref(),
            &state.editor.project.levels,
            &format!(
                "The level each room of {module} aims at in the bake, of its {} level, set in kjk's level editor.",
                state.bake_entry.of
            ),
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        if state.bake_entry.levels.is_none() {
            let bake = folder.join("bake.toml");
            let text = std::fs::read_to_string(&bake)
                .map_err(|error| format!("{}: {error}", bake.display()))?;
            let text = level_edit::levels::with_levels_file(&text, &module, &file)?;
            std::fs::write(&bake, &text).map_err(|error| format!("{}: {error}", bake.display()))?;
            state.bake_entry = level_edit::levels::bake_entry(&text, &module);
        }
        state.levels_original = state.editor.project.levels.clone();
        state.say(format!("saved the rooms' levels to {}", path.display()));
    }
    // An installed level's lights placed by hand go to the light data's
    // `fittings.toml`, where the game reads them.
    let project = &state.editor.project;
    if project.map.is_none()
        && let Some(folder) = &state.setup.light_data
    {
        let path = folder.join("fittings.toml");
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let lights = &project.kjk.lights;
        let had = level_edit::lights::read_placed(&text, &project.game, &project.module)?;
        if had != *lights {
            let new =
                level_edit::lights::write_placed(&text, &project.game, &project.module, lights)?;
            std::fs::write(&path, new).map_err(|error| format!("{}: {error}", path.display()))?;
            let message = format!(
                "saved {} lights placed by hand to {}",
                lights.len(),
                path.display()
            );
            state.say(message);
        }
    }
    // And its lamps, to the same file.
    let project = &state.editor.project;
    if project.map.is_none()
        && let Some(folder) = &state.setup.light_data
    {
        let path = folder.join("fittings.toml");
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let lamps = &project.kjk.lamps;
        let had = level_edit::lamps::read_lamps(&text, &project.game, &project.module)?;
        if had != *lamps {
            let new = level_edit::lamps::write_lamps(&text, &project.game, &project.module, lamps)?;
            std::fs::write(&path, new).map_err(|error| format!("{}: {error}", path.display()))?;
            let message = format!("saved {} lamps to {}", lamps.len(), path.display());
            state.say(message);
        }
    }
    Ok(())
}

/// `edit` commands from `--cmds`, in order.
pub(crate) fn commands_from_console(
    mut state: ResMut<EditorState>,
    mut requests: ResMut<console::EditRequests>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let state = &mut *state;
    for words in std::mem::take(&mut requests.0) {
        if let ["pick", x, y] = words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            let (Ok(x), Ok(y)) = (number(x), number(y)) else {
                state.say("edit pick takes a point of the view in pixels");
                continue;
            };
            state.pending_pick = Some(Vec2::new(x, y));
            state.pending_pick_adds = false;
            continue;
        }
        // The window's own state (`ui`): its tool, the picker's tab and
        // search, for scripted screenshots.
        if words.first().is_some_and(|word| word == "ui") {
            state.pending_ui.push(words[1..].to_vec());
            continue;
        }
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        run(state, &words, &cameras);
    }
}

pub(crate) fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Creature => "creature",
        Kind::Door => "door",
        Kind::Placeable => "placeable",
        Kind::Trigger => "trigger",
        Kind::Waypoint => "waypoint",
        Kind::Sound => "sound",
        Kind::Encounter => "encounter",
        Kind::Store => "store",
    }
}

/// What the panel and a dump say about a target.
pub(crate) fn describe(state: &EditorState, target: Target) -> serde_json::Value {
    let placement = state.editor.placement(target);
    let (position, facing) = placement.unwrap_or(([0.0; 3], 0.0));
    match target {
        Target::Object(kind, index) => {
            let entry = state.editor.entry(kind, index);
            // What its template makes it do.
            let behaviour = entry.as_ref().and_then(|entry| {
                let gff = state.behaviour_source(kind, entry)?;
                let own = state.editor.project.templates.get(&entry.template);
                let scripts: BTreeMap<String, String> = level_edit::template::scripts(&gff)
                    .into_iter()
                    .filter(|(_, script)| !script.is_empty())
                    .collect();
                Some(serde_json::json!({
                    "project_template": own.is_some(),
                    "copied_from": own.map(|made| made.from.clone()),
                    "scripts": scripts,
                    "useable": level_edit::template::flag(&gff, "Useable"),
                    "holds_items": level_edit::template::flag(&gff, "HasInventory"),
                    "items": level_edit::template::items(&gff),
                }))
            });
            serde_json::json!({
                "kind": kind_name(kind),
                "index": index,
                "template": entry.as_ref().map(|e| e.template.clone()),
                "tag": entry.as_ref().map(|e| state.tag_of(e)),
                "position": position,
                "facing": facing,
                "editable": EDITABLE.contains(&kind),
                "behaviour": behaviour,
            })
        }
        Target::Prop(at) => {
            let prop = state.editor.project.kjk.props.get(at);
            // Where its model's corners stand, lowest and highest.
            let bounds = prop.and_then(|prop| {
                let model = state.prop_models.get(&prop.model)?.as_ref().ok()?;
                let points = model.ir.meshes.iter().flat_map(|mesh| &mesh.positions);
                kjk_game::scene::span(points.map(|point| {
                    let ja = point.map(|value| value / collision::UNIT);
                    Vec3::from_array(prop.place(ja))
                }))
                .map(|(low, high)| [low.to_array(), high.to_array()])
            });
            serde_json::json!({
                "bounds": bounds,
                "kind": "prop",
                "index": at,
                "name": prop.map(|p| p.name.clone()),
                "model": prop.map(|p| p.model.clone()),
                "scale": prop.map(|p| p.scale),
                "solid": prop.map(|p| p.solid),
                "position": position,
                "facing": facing,
            })
        }
        Target::Box(at) => {
            let shape = state.editor.project.kjk.boxes.get(at);
            serde_json::json!({
                "kind": "box",
                "index": at,
                "name": shape.map(|b| b.name.clone()),
                "size": shape.map(|b| b.size),
                "role": shape.map(|b| b.role.word()),
                "position": position,
                "facing": facing,
            })
        }
        Target::Room(at) => {
            let project = &state.editor.project;
            let room = project.map.as_ref().and_then(|map| map.rooms.get(at));
            serde_json::json!({
                "kind": "room",
                "index": at,
                "name": level_edit::map::room_name(&project.module, at),
                "model": room.map(|room| room.model.clone()),
                "from": room.map(|room| room.from),
                "position": position,
                "facing": facing,
            })
        }
        Target::Entry => serde_json::json!({
            "kind": "entry",
            "position": position,
            "facing": facing,
        }),
        Target::Light(at) => {
            let light = state.editor.project.kjk.lights.get(at);
            serde_json::json!({
                "kind": "light",
                "index": at,
                "position": position,
                "colour": light.map(|l| l.colour),
                "radius": light.map(|l| l.radius),
                "aim": light.map(|l| l.aim),
                "cone": light.map(|l| l.cone),
                "half": light.map(|l| l.half),
                "reason": light.map(|l| l.reason.clone()),
            })
        }
        Target::Sun => {
            let sun = state.editor.sun();
            let middle = state.editor.level_middle;
            let seen = sun.map(|sun| sun.seen_from(middle));
            serde_json::json!({
                "kind": "sun",
                "the_projects": state.editor.project.kjk.sun.is_some(),
                "at": sun.map(|sun| sun.at),
                "bearing": seen.map(|seen| seen.0),
                "height": seen.map(|seen| seen.1),
                "distance": seen.map(|seen| seen.2),
                "colour": sun.map(|sun| sun.colour),
                "multiplier": sun.map(|sun| sun.multiplier),
                "radius": sun.map(|sun| sun.radius),
                "level_middle": middle,
            })
        }
        Target::Lamp(at) => {
            let lamp = state.editor.project.kjk.lamps.get(at);
            let drawn = (state.drawn.things.iter())
                .any(|(target, models)| *target == Target::Lamp(at) && !models.is_empty());
            serde_json::json!({
                "kind": "lamp",
                "index": at,
                "position": position,
                "facing": facing,
                "model": lamp.map(|l| l.model.clone()),
                "mesh": lamp.map(|l| l.mesh.clone()),
                "on": lamp.map(|l| if l.on_placeable { "placeable" } else { "room" }),
                "reason": lamp.map(|l| l.reason.clone()),
                "drawn": drawn,
            })
        }
        Target::Fitted(at) => {
            let seen = state.editor.seen_lights.get(at);
            serde_json::json!({
                "kind": "fitted light",
                "index": at,
                "position": position,
                "source": seen.map(|(_, source)| source.clone()),
                "colour": seen.map(|(l, _)| l.colour),
                "radius": seen.map(|(l, _)| l.radius),
            })
        }
    }
}

pub(crate) fn publish(state: Res<EditorState>, mut facts: ResMut<DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    facts.0.insert("editor".into(), facts_of(&state));
}

/// Everything a dump says of the editor (its `editor` fact), which the
/// agent link writes too (`level`): the project, every object, the
/// selection, the props and boxes, the lighting, the navigation mesh, the
/// scripts and conversations, the messages and the project's problems.
pub(crate) fn facts_of(state: &EditorState) -> serde_json::Value {
    let project = &state.editor.project;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let entries = state.editor.entries();
    for entry in &entries {
        *counts.entry(kind_name(entry.kind)).or_insert(0) += 1;
    }
    let objects: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "kind": kind_name(entry.kind),
                "index": entry.index,
                "template": entry.template,
                "tag": state.tag_of(entry),
                "position": entry.position,
                "facing": entry.facing,
            })
        })
        .collect();
    serde_json::json!({
            "project": project.name,
            "folder": project.folder,
            "game": project.game,
            "module": project.module,
            "area": project.area,
            "unsaved": state.unsaved(),
            "undo": state.editor.undo_depth(),
            "redo": state.editor.redo_depth(),
            "counts": counts,
            "objects": objects,
            "selected": state.editor.selected.map(|target| describe(state, target)),
            "props": (0..project.kjk.props.len()).map(|at| describe(state, Target::Prop(at))).collect::<Vec<_>>(),
            "boxes": (0..project.kjk.boxes.len()).map(|at| describe(state, Target::Box(at))).collect::<Vec<_>>(),
            "lighting": {
                "view": format!("{:?}", state.light_view),
                "bake": state.bake.as_ref().map(|bake| bake.display().to_string()),
                "placed": (0..project.kjk.lights.len()).map(|at| describe(state, Target::Light(at))).collect::<Vec<_>>(),
                "lamps": (0..project.kjk.lamps.len()).map(|at| describe(state, Target::Lamp(at))).collect::<Vec<_>>(),
                "sun": describe(state, Target::Sun),
                "lamps_offered": state.lamp_library.found.len(),
                "fitted": state.editor.seen_lights.len(),
                "fitted_by_source": state.editor.seen_lights.iter().fold(BTreeMap::<String, usize>::new(), |mut counts, (_, source)| {
                    *counts.entry(source.clone()).or_insert(0) += 1;
                    counts
                }),
                "light_data": state.setup.light_data.as_ref().map(|folder| folder.display().to_string()),
            },
            "nav": state.nav_facts.as_ref().map(|nav| serde_json::json!({
                "faces": nav.faces,
                "islands": nav.islands,
                "portals": nav.portals,
                "from": nav.from,
                "apart": nav.apart,
                "entry_off": nav.entry_off,
                "by_links": nav.by_links,
                "links": nav.links.iter().map(|check| match check {
                    Ok(()) => "crossed".to_string(),
                    Err(why) => why.clone(),
                }).collect::<Vec<_>>(),
            })),
            "prop_models": state.prop_models.iter().map(|(path, read)| match read {
                Ok(model) => serde_json::json!({
                    "model": path,
                    "meshes": model.ir.meshes.iter().map(|mesh| mesh.texture.clone()).collect::<Vec<_>>(),
                    "textures": model.textures.iter().map(|(name, texture)| {
                        let texels = texture.mips.first().map_or(&[][..], Vec::as_slice);
                        let mean = texels.chunks(4).fold([0u64; 3], |sum, t| [sum[0] + u64::from(t[0]), sum[1] + u64::from(t[1]), sum[2] + u64::from(t[2])]);
                        let count = (texels.len() / 4).max(1) as u64;
                        serde_json::json!({"name": name, "size": [texture.width, texture.height], "mean": mean.map(|c| c / count)})
                    }).collect::<Vec<_>>(),
                }),
                Err(why) => serde_json::json!({"model": path, "error": why}),
            }).collect::<Vec<_>>(),
            "drawn_props": state.drawn.things.iter().filter(|(t, models)| matches!(t, Target::Prop(_)) && !models.is_empty()).count(),
            "collision_overlay": state.drawn.overlay.is_some(),
            "templates": { "placeables": state.setup.placeables.len(), "creatures": state.setup.creatures.len(), "ja_models": state.setup.models.len() },
            "messages": state.messages,
            "audit": state.audit,
            "redraws": state.redraws,
            "problems": project.problems().into_iter().map(|p| p.what).collect::<Vec<_>>(),
            "conversations": project.conversations.keys().collect::<Vec<_>>(),
            "project_scripts": state.project_scripts,
            "script_errors": state.script_errors,
            "nav_links": project.kjk.links.iter().map(|link| serde_json::json!({
                "kind": format!("{:?}", link.kind),
                "from": link.from,
                "to": link.to,
            })).collect::<Vec<_>>(),
            "room_levels": project.levels,
            "other_areas": project.others.keys().collect::<Vec<_>>(),
            "levels_of_the_project": project.level_list(),
    })
}
