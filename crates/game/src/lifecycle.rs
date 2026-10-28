//! Module installs in `kjk` (I14): the `Installer` opens a module through
//! the install's resolver and plans it (`session::plan`: the sim's level)
//! with its scene (what the view draws), the loading screen's image and
//! title; the hosts (`sim_host`, `kjk move run`, the `kjk lifecycle`
//! verbs) commit what it prepared. Nothing here touches a running world.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use asset_odyssey::module::ModuleIr;
use asset_odyssey::texture::TextureIr;
use session::{Plan, Request};
use sim::motion::AnimTable;

use vfs::resolver::Resolver;
use vfs::{DirIndex, Discovery, Game};

use crate::installs::{self, Install};
use crate::modules::ResolverSource;
use crate::viewer::{self, LoadedScene};

/// Whether this process's plans open the ways the games keep sealed
/// (`data/links`, `session::Overrides::playtest_links`): set once at start
/// by `--playtest-links`, so every module a session goes to plans alike.
static PLAYTEST_LINKS: AtomicBool = AtomicBool::new(false);

/// Opens the sealed ways (`--playtest-links`) for every plan from now on.
pub fn open_playtest_links() {
    PLAYTEST_LINKS.store(true, Ordering::Relaxed);
}

/// What this process's plans apply over the module's own data.
fn overrides(encounters: bool) -> session::Overrides {
    session::Overrides {
        encounters,
        playtest_links: PLAYTEST_LINKS.load(Ordering::Relaxed),
    }
}

/// Opens and plans the modules of one install.
pub struct Installer {
    pub game: Game,
    /// The install, opened once a process (`installs`): every module's
    /// resolver stacks on its base, and the loading screens' images and the
    /// bar are in it (`swpc_tex_gui.erf`), so the screen shows at once.
    install: Arc<Install>,
    pub anims: Arc<AnimTable>,
    /// The areas its plans built (D13's level cache): planning a module
    /// again (a host's own client, a module gone back to) takes them.
    areas: session::AreaCache,
}

/// What a loading screen shows: the module's image, the progress bar's
/// fill, a title.
pub struct LoadingScreen {
    pub image: Option<TextureIr>,
    /// The image's resref, for the game's own screen (`ui_view`).
    pub name: Option<String>,
    pub bar: Option<TextureIr>,
    pub title: Option<String>,
}

/// The resources of the module a scene is in, for the views that read what
/// a module may carry itself (its own templates, models and textures, as
/// the planner reads its objects'): the install's base layers under the
/// module's archives, opened by the first read that needs them (on any
/// thread: clones share it), else the base alone.
#[derive(Clone, Default)]
pub struct ModuleResolver {
    base: Option<Arc<Resolver>>,
    open: Option<Open>,
    module: Option<(String, Opened)>,
}

/// What opens a module's resolver by its name.
type Open = Arc<dyn Fn(&str) -> Option<Resolver> + Send + Sync>;

/// A module's resolver, opened by the first read that needs it (`None`:
/// its archives did not open).
type Opened = Arc<std::sync::OnceLock<Option<Arc<Resolver>>>>;

impl ModuleResolver {
    /// The base's resources, and `installer`'s modules'.
    pub fn new(base: Option<Arc<Resolver>>, installer: Option<Arc<Installer>>) -> ModuleResolver {
        let open = installer
            .map(|installer| Arc::new(move |name: &str| installer.resolver(name).ok()) as Open);
        ModuleResolver::opening(base, open)
    }

    /// The base's resources, and the modules' as `open` opens them.
    pub fn opening(base: Option<Arc<Resolver>>, open: Option<Open>) -> ModuleResolver {
        ModuleResolver {
            base,
            open,
            module: None,
        }
    }

    /// Follows the scene into module `name`: whether it is another module
    /// than before (what was read before is then stale, as a model one
    /// module has may differ from another's of the name).
    pub fn enter(&mut self, name: &str) -> bool {
        if self.module.as_ref().is_some_and(|(at, _)| at == name) {
            return false;
        }
        self.module = Some((name.to_string(), Arc::default()));
        true
    }

    /// The current module's resolver, else the base's (`None`: no install).
    pub fn get(&self) -> Option<Arc<Resolver>> {
        let opened =
            self.open
                .as_ref()
                .zip(self.module.as_ref())
                .and_then(|(open, (name, opened))| {
                    opened.get_or_init(|| open(name).map(Arc::new)).clone()
                });
        opened.or_else(|| self.base.clone())
    }

    /// Whether there is anything to read from.
    pub fn is_some(&self) -> bool {
        self.base.is_some()
    }
}

/// The module resources' probes (`kjk sim probes --only "module
/// resources"`), on K1's data where this machine has the install: a robe
/// only the Endar Spire's archives carry is read there (its armour and
/// the body it dresses), not through the base, nor once the scene is in
/// another module.
#[cfg(feature = "probes")]
pub fn module_resolver_probes() -> sim::probe::Checks {
    use sim::probe::check;
    const ROBE: &str = "g_a_jedirobe002";
    // `appearance.2da`'s P_FEM_A_MED_01, a player's body.
    const PLAYER: u16 = 92;
    let Some(index) = crate::k1_index() else {
        return vec![("K1 install", check(true, "none on this machine".into()))];
    };
    let base = crate::k1_open(&index, None).ok().map(Arc::new);
    let open = Arc::new(move |name: &str| crate::k1_open(&index, Some(name)).ok()) as Open;
    let mut resources = ModuleResolver::opening(base.clone(), Some(open));
    let has = |resolver: Option<Arc<Resolver>>| {
        resolver.is_some_and(|resolver| resolver.read(ROBE, fmt_odyssey::ResType::UTI).is_some())
    };
    let in_base = has(base);
    let entered = resources.enter("end_m01aa");
    let again = resources.enter("end_m01aa");
    let in_module = has(resources.get());
    let dressed = resources.get().and_then(|resolver| {
        let mut tables = asset_odyssey::tables::Tables::default();
        crate::persona::wear(&resolver, &mut tables, (PLAYER, Some(ROBE.into())))
    });
    let left = resources.enter("tar_m02aa");
    let elsewhere = has(resources.get());
    vec![
        (
            "a module's own armour read through its archives",
            check(
                !in_base && entered && !again && in_module && dressed.is_some(),
                format!(
                    "{ROBE}: base {in_base}, end_m01aa {in_module} (entered {entered}, \
                     again {again}), dressed as {:?}",
                    dressed.map(|wear| wear.body)
                ),
            ),
        ),
        (
            "another module reads its own",
            check(
                left && !elsewhere,
                format!("tar_m02aa: entered {left}, {ROBE} {elsewhere}"),
            ),
        ),
    ]
}

/// A module planned, with its scene when the host draws one.
pub struct Prepared {
    pub plan: Plan,
    pub scene: Option<LoadedScene>,
    /// Milliseconds: opening the module's archives, the plan, the scene.
    pub open_ms: f64,
    pub plan_ms: f64,
    pub scene_ms: f64,
}

impl Prepared {
    /// Its parts as launch phases under `under` (`launch_times`).
    pub fn record(&self, under: &str) {
        let facts = &self.plan.facts;
        let record =
            |phase: &str, ms: f64| crate::launch_times::record(&format!("{under}/{phase}"), ms);
        record("archives", self.open_ms);
        record("plan", self.plan_ms);
        record("plan/module and objects", facts.read_ms);
        record("plan/collision", facts.collision_ms);
        record("plan/templates", facts.items_ms);
        record("plan/scripts", facts.script_ms);
        record("scene", self.scene_ms);
    }
}

impl Installer {
    /// Every module of the install, by name.
    #[cfg(feature = "probes")]
    pub fn modules(&self) -> Vec<String> {
        Resolver::modules(&self.install.index)
    }

    /// The installer of `game`'s install, its plans taking the import
    /// step's products when the install was imported (`kjk import`).
    pub fn new(discovery: &Discovery, game: Game, anims: Arc<AnimTable>) -> Result<Self, String> {
        Installer::open(discovery, game, anims, true)
    }

    /// The same, reading and building everything from the install itself
    /// for every plan (no store; for comparing).
    pub fn without_import(
        discovery: &Discovery,
        game: Game,
        anims: Arc<AnimTable>,
    ) -> Result<Self, String> {
        Installer::open(discovery, game, anims, false)
    }

    fn open(
        discovery: &Discovery,
        game: Game,
        anims: Arc<AnimTable>,
        imported: bool,
    ) -> Result<Self, String> {
        let install = installs::open(discovery, game)?;
        let areas = crate::import_verbs::area_cache(&install, imported);
        Ok(Installer {
            game,
            install,
            anims,
            areas,
        })
    }

    /// Whether its plans take the import step's products.
    pub fn imported(&self) -> bool {
        self.areas.imported().is_some()
    }

    /// Whether its plans keep and take areas in the store.
    pub fn bakes(&self) -> bool {
        self.areas.bakes()
    }

    /// The install's files.
    pub(crate) fn index(&self) -> &DirIndex {
        &self.install.index
    }

    /// The install's resolver without a module.
    pub fn base_resolver(&self) -> Result<&Resolver, String> {
        self.install.base()
    }

    pub fn key(&self) -> &'static str {
        self.game.key()
    }

    /// The install's resolver with `module` loaded: the module's archives
    /// on the install's base layers, which are opened once and shared.
    pub fn resolver(&self, module: &str) -> Result<Resolver, String> {
        self.install.resolver(Some(module))
    }

    /// A request for `module` of this installer's game, for a world of
    /// `seats` players.
    pub fn request(&self, module: &str, arrival: Option<&str>, seats: u8) -> Request {
        Request::new(self.key(), module, arrival, seats)
    }

    /// The request a running world (`snapshot`) makes for `module`: room
    /// for its seats, planned for `Snapshot::players_for` (I30 P21). That
    /// is the players driving a body now, or the count the module was
    /// planned for when the world has been there. Every plan made from a
    /// running world goes through it; `request` alone plans for one.
    pub fn request_from(
        &self,
        snapshot: &sim::Snapshot,
        module: &str,
        arrival: Option<&str>,
    ) -> Request {
        self.request(module, arrival, snapshot.seats())
            .with_players(snapshot.players_for(module))
    }

    /// Plans `request`, and builds its scene when `scene` is asked for.
    /// `request`'s plan without the module's encounter overrides (I28): the
    /// module as the game has it, for before-and-after reports.
    #[cfg(feature = "probes")]
    pub fn plan_without_encounters(&self, request: &Request) -> Result<session::Plan, String> {
        let resolver = self.resolver(&request.module)?;
        session::plan_with(
            &ResolverSource(&resolver),
            request,
            self.anims.clone(),
            overrides(false),
            Some(&self.areas),
        )
        .map_err(|error| error.to_string())
    }

    /// `module` planned for one seat arriving at `arrive` (the module's
    /// entry without one).
    pub fn prepare_one(&self, module: &str, arrive: Option<&str>) -> Result<Prepared, String> {
        self.prepare(&self.request(module, arrive, 1), false)
    }

    pub fn prepare(&self, request: &Request, scene: bool) -> Result<Prepared, String> {
        self.prepare_with(request, scene, &|_| {})
    }

    /// The same, saying how far it got (0 to 1) as it goes.
    pub fn prepare_with(
        &self,
        request: &Request,
        scene: bool,
        progress: &dyn Fn(f32),
    ) -> Result<Prepared, String> {
        let started = Instant::now();
        let resolver = self.resolver(&request.module)?;
        let open_ms = started.elapsed().as_secs_f64() * 1000.0;
        progress(0.25);
        // The scene needs only the module's IR, read again for it (a few
        // milliseconds): it is built beside the plan rather than after.
        let ((plan, plan_ms), scene) = std::thread::scope(|scope| {
            let scene = scene.then(|| scope.spawn(|| self.scene(&resolver, &request.module)));
            let started = Instant::now();
            let plan = session::plan_with(
                &ResolverSource(&resolver),
                request,
                self.anims.clone(),
                overrides(true),
                Some(&self.areas),
            );
            let plan_ms = started.elapsed().as_secs_f64() * 1000.0;
            let scene =
                scene.map(|building| building.join().expect("the scene's thread does not panic"));
            ((plan, plan_ms), scene)
        });
        let plan = plan.map_err(|error| error.to_string())?;
        let (scene, scene_ms) = match scene {
            Some(built) => {
                let (mut scene, ms) = built?;
                scene.cutscene_models = self.cutscene_models(&resolver, &plan.level);
                (Some(scene), ms)
            }
            None => (None, 0.0),
        };
        progress(0.9);
        Ok(Prepared {
            plan,
            scene,
            open_ms,
            plan_ms,
            scene_ms,
        })
    }

    /// The camera models the level's conversations name and their
    /// cutscenes' stunt models, by lowercase name (those the install has).
    fn cutscene_models(
        &self,
        resolver: &Resolver,
        level: &sim::Level,
    ) -> std::collections::BTreeMap<String, std::sync::Arc<asset_odyssey::model::ModelIr>> {
        let names: std::collections::BTreeSet<String> = level
            .scripts()
            .into_iter()
            .flat_map(|library| library.dialogues())
            .flat_map(|(_, dlg)| {
                let stunts = dlg.stunts.iter().filter(|_| dlg.staged());
                std::iter::once(&dlg.camera_model).chain(stunts.map(|(_, model)| model))
            })
            .map(|name| name.to_ascii_lowercase())
            .filter(|name| !name.is_empty())
            .collect();
        names
            .into_iter()
            .filter_map(|name| {
                let model = viewer::model(resolver, &name).ok()??;
                Some((name, std::sync::Arc::new(model)))
            })
            .collect()
    }

    /// `module`'s entry area as a scene, with its sound read, and how long
    /// it took (ms).
    fn scene(&self, resolver: &Resolver, module: &str) -> Result<(LoadedScene, f64), String> {
        let started = Instant::now();
        let ir = ModuleIr::build(module, &ResolverSource(resolver))
            .map_err(|refusal| format!("{module}: {refusal:?}"))?;
        // The plan's entry area (`session::plan_with`).
        let area = ir
            .areas
            .iter()
            .position(|area| area.resref == ir.entry_area)
            .or((!ir.areas.is_empty()).then_some(0))
            .ok_or(format!("{module}: no area"))?;
        let resref = &ir.areas[area].resref;
        let (scene, sound) = std::thread::scope(|scope| {
            let sound = scope.spawn(|| {
                crate::launch_times::timed("kotor stage/sound", || {
                    crate::ambience::preread(resolver, module, resref)
                })
            });
            let scene = viewer::module_scene(resolver, self.game, &ir, area);
            (
                scene,
                sound.join().expect("the sound's thread does not panic"),
            )
        });
        let mut scene = scene?;
        scene.sound = sound;
        Ok((scene, started.elapsed().as_secs_f64() * 1000.0))
    }

    /// The level an announcement names, built here and checked against it.
    #[cfg(feature = "probes")]
    pub fn level_for(&self, announcement: &sim::Announcement) -> Result<sim::Level, String> {
        let prepared = self.prepare(&Request::of(announcement), false)?;
        prepared
            .plan
            .check(announcement)
            .map_err(|mismatch| mismatch.to_string())?;
        Ok(prepared.plan.level)
    }

    /// A strref's text from `dialog.tlk`.
    pub fn text(&self, strref: u32) -> Option<String> {
        self.install
            .talk()?
            .text(strref)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    /// What the loading screen shows for `module`: its image (reone's
    /// `load_<module>`, else the area's `loadscreens.2da` row), the bar's
    /// fill (`loadscreen.gui`'s `bluefill`), the area's name.
    pub fn loading_screen(&self, module: &str) -> LoadingScreen {
        let global = self.install.base().ok();
        let bar = global.and_then(|resolver| viewer::texture(resolver, "bluefill"));
        let name = format!("load_{module}");
        if let Some(image) = global.and_then(|resolver| viewer::texture(resolver, &name)) {
            return LoadingScreen {
                image: Some(image),
                name: Some(name),
                bar,
                title: None,
            };
        }
        let Ok(resolver) = self.resolver(module) else {
            return LoadingScreen {
                image: None,
                name: None,
                bar: None,
                title: None,
            };
        };
        let source = ResolverSource(&resolver);
        let ir = asset_odyssey::module::ModuleIr::build(module, &source).ok();
        let area = ir
            .as_ref()
            .and_then(|ir| ir.areas.iter().find(|a| a.resref == ir.entry_area));
        let named = viewer::texture(&resolver, &name).map(|image| (image, name));
        let found = named.or_else(|| {
            let row = area.and_then(|area| area.load_screen).unwrap_or(0);
            let name = resolver
                .two_da("loadscreens")?
                .resref(row as usize, "bmpresref")?;
            Some((viewer::texture(&resolver, &name)?, name))
        });
        let title = area
            .and_then(|area| area.name_strref)
            .and_then(|strref| self.text(strref));
        let (image, name) = found.unzip();
        LoadingScreen {
            image,
            name,
            bar,
            title,
        }
    }
}
