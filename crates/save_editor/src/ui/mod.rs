//! The editor's window: the saves on the left, the install and the party
//! along the top, a page per tab in the middle, the selected row's details
//! on the right, and the changes at the bottom.

mod icons;
mod list;
mod pages;
mod shell;
mod shot;
mod style;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use edit::Who;
use save_editor::model::catalog::Catalog;
use save_editor::model::locate::{self, SaveRow};
use save_editor::model::session::{Change, Ground, Session};
use vfs::Game;

/// The pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Equipment,
    Inventory,
    Powers,
    Stats,
    Party,
    Quests,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Equipment,
        Tab::Inventory,
        Tab::Powers,
        Tab::Stats,
        Tab::Party,
        Tab::Quests,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Equipment => "Equipment",
            Tab::Inventory => "Inventory",
            Tab::Powers => "Powers & Feats",
            Tab::Stats => "Stats",
            Tab::Party => "Party",
            Tab::Quests => "Quests & Flags",
        }
    }

    fn from_name(name: &str) -> Option<Tab> {
        Tab::ALL
            .into_iter()
            .find(|tab| tab.title().to_lowercase().starts_with(&name.to_lowercase()))
    }
}

/// An install's names being read, or read.
enum Loading {
    Pending(Receiver<Result<Catalog, String>>),
    Ready(Result<Box<Catalog>, String>),
}

/// A character in the party strip: who, as named and pictured.
pub struct Member {
    pub who: Who,
    pub name: String,
    pub portrait: Option<String>,
    pub character: edit::Character,
}

/// The save open, with what is made of it.
pub struct Open {
    pub row: SaveRow,
    pub session: Session,
    pub members: Vec<Member>,
    level: Option<Receiver<Result<sim::Level, String>>>,
}

/// The app eframe runs.
pub struct App {
    shot: shot::Shot,
    /// Install folders picked here, by their variables' names.
    chosen: BTreeMap<String, String>,
    discovery: vfs::Discovery,
    catalogs: HashMap<Game, Loading>,
    /// The saves' folder, and what it was found to be.
    root: Option<PathBuf>,
    found: String,
    rows: Vec<SaveRow>,
    open: Option<Open>,
    /// Why the last save picked did not open.
    refused: Option<String>,
    tab: Tab,
    search: String,
    focus_search: bool,
    selected: Option<u64>,
    /// The party strip's pick.
    member: usize,
    /// The last thing that went wrong, one line in red.
    note: Option<String>,
    icons: icons::Icons,
    pages: pages::State,
}

impl App {
    pub fn new(ctx: &egui::Context) -> App {
        style::compact(ctx);
        let root = locate::default_saves();
        let mut app = App {
            shot: shot::Shot::from_env(),
            chosen: BTreeMap::new(),
            discovery: locate::discovery(),
            catalogs: HashMap::new(),
            root: None,
            found: String::new(),
            rows: Vec::new(),
            open: None,
            refused: None,
            tab: Tab::Inventory,
            search: String::new(),
            focus_search: false,
            selected: None,
            member: 0,
            note: None,
            icons: icons::Icons::default(),
            pages: pages::State::default(),
        };
        match root {
            Some(root) => app.use_root(root),
            None => app.found = "kjk's saves folder is not known here: choose one".into(),
        }
        for (game, _) in locate::installs(&app.discovery) {
            app.load_catalog(game);
        }
        app.apply_env();
        app
    }

    /// The state the environment asks for (for pictures of the window):
    /// `KJK_EDITOR_OPEN` (a save id), `KJK_EDITOR_TAB`, `KJK_EDITOR_SEARCH`,
    /// `KJK_EDITOR_MEMBER` (the party strip's pick) and `KJK_EDITOR_SELECT` (a
    /// row's key).
    fn apply_env(&mut self) {
        if let Ok(id) = std::env::var("KJK_EDITOR_OPEN") {
            self.open_save(&id);
        }
        if let Some(tab) = std::env::var("KJK_EDITOR_TAB")
            .ok()
            .and_then(|name| Tab::from_name(&name))
        {
            self.tab = tab;
        }
        if let Ok(search) = std::env::var("KJK_EDITOR_SEARCH") {
            self.search = search;
        }
        let number = |name: &str| std::env::var(name).ok()?.parse::<u64>().ok();
        if let Some(member) = number("KJK_EDITOR_MEMBER") {
            self.member = member as usize;
        }
        if let Some(key) = number("KJK_EDITOR_SELECT") {
            self.selected = Some(key);
            self.pages.select(key);
        }
    }

    /// `root` as the saves' folder, listed.
    fn use_root(&mut self, root: PathBuf) {
        self.found = locate::check_path(&root).to_string();
        self.rows = locate::list(&root);
        self.root = Some(root);
    }

    /// `game`'s names read in the background.
    fn load_catalog(&mut self, game: Game) {
        let (send, receive) = channel();
        let chosen = self.chosen.clone();
        std::thread::spawn(move || {
            let _ = send.send(Catalog::load(&locate::discovery_over(&chosen), game));
        });
        self.catalogs.insert(game, Loading::Pending(receive));
    }

    /// Catalogs and levels that finished reading.
    fn receive(&mut self) {
        let mut arrived = false;
        for loading in self.catalogs.values_mut() {
            if let Loading::Pending(receive) = loading
                && let Ok(result) = receive.try_recv()
            {
                *loading = Loading::Ready(result.map(Box::new));
                arrived = true;
            }
        }
        if let Some(open) = &mut self.open
            && let Some(receive) = &open.level
            && let Ok(result) = receive.try_recv()
        {
            open.level = None;
            match result {
                Ok(level) => {
                    open.session.level = Some(level);
                }
                Err(why) => self.note = Some(format!("its module did not plan: {why}")),
            }
            arrived = true;
        }
        if arrived {
            self.refresh_members();
        }
    }

    /// The game the open save is of (none when it does not say: never
    /// another game's install for it); with no save open, the first install
    /// found.
    pub fn game(&self) -> Option<Game> {
        match &self.open {
            Some(open) => open.row.game,
            None => locate::installs(&self.discovery)
                .first()
                .map(|(game, _)| *game),
        }
    }

    /// The names of the game in use, when read.
    pub fn catalog(&self) -> Option<&Catalog> {
        ready(&self.catalogs, self.game())
    }

    /// Why there are no names to show, if there are none.
    pub fn catalog_missing(&self) -> Option<String> {
        let Some(game) = self.game() else {
            return Some("no K1 or TSL install found: resrefs are shown instead of names".into());
        };
        let name = locate::game_name(game);
        match self.catalogs.get(&game) {
            None => Some(format!(
                "no {name} install found: resrefs are shown instead of names"
            )),
            Some(Loading::Pending(_)) => Some(format!("reading the {name} install…")),
            Some(Loading::Ready(Err(why))) => {
                Some(format!("the {name} install did not read: {why}"))
            }
            Some(Loading::Ready(Ok(_))) => None,
        }
    }

    /// Save `id` of the saves' folder opened, or why not.
    fn open_save(&mut self, id: &str) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(row) = self.rows.iter().find(|row| row.id == id).cloned() else {
            self.refused = Some(format!("no save {id} in this folder"));
            return;
        };
        match edit::open(&root, id) {
            Ok(opened) => {
                let level = row.game.and_then(|game| self.discovery.get(game)).map(|_| {
                    let (send, receive) = channel();
                    let (save, chosen) = (opened.save.clone(), self.chosen.clone());
                    std::thread::spawn(move || {
                        let _ = send.send(edit::level_of(&save, &locate::discovery_over(&chosen)));
                    });
                    receive
                });
                self.open = Some(Open {
                    row,
                    session: Session::new(opened),
                    members: Vec::new(),
                    level,
                });
                self.refused = None;
                self.member = 0;
                self.selected = None;
                self.note = None;
                self.refresh_members();
            }
            Err(refusal) => {
                self.refused = Some(format!("{}: {refusal}", row.name));
            }
        }
    }

    /// The party strip made again from the edited save.
    fn refresh_members(&mut self) {
        let catalog = ready(&self.catalogs, self.game());
        let Some(open) = &mut self.open else {
            return;
        };
        let player = open.session.opened.meta.character.as_ref();
        open.members = open
            .session
            .characters()
            .into_iter()
            .map(|character| {
                let (name, portrait) = match character.who {
                    Who::Player(seat) => match (seat.0, player) {
                        (0, Some(desc)) if !desc.name.is_empty() => {
                            (desc.name.clone(), Some(desc.portrait.clone()))
                        }
                        _ => (format!("Player {}", seat.0 + 1), None),
                    },
                    Who::Companion { npc, .. } | Who::Away { npc } => {
                        // The install's template, else the one the party
                        // table keeps (a template only one module holds).
                        let kept = open.session.edited.script.party.available.get(&npc);
                        let creature = catalog.map(|catalog| {
                            let creature = catalog.creature(&character.template);
                            match kept.and_then(|npc| npc.template_data.as_deref()) {
                                Some(bytes) if creature.name.is_empty() => {
                                    catalog.creature_from(bytes)
                                }
                                _ => creature,
                            }
                        });
                        match creature {
                            Some(creature) if !creature.name.is_empty() => {
                                (creature.name, creature.portrait)
                            }
                            _ => (character.template.clone(), None),
                        }
                    }
                };
                Member {
                    who: character.who,
                    name,
                    portrait,
                    character,
                }
            })
            .collect();
        if self.member >= open.members.len() {
            self.member = 0;
        }
    }

    /// `change` made to the open save, or why not in the note line.
    pub fn apply(&mut self, change: Change) {
        let result = {
            let catalog = ready(&self.catalogs, self.game());
            let Some(open) = &mut self.open else {
                return;
            };
            open.session.apply(change, &Ground::of(catalog))
        };
        self.after(result);
    }

    /// The last change taken back.
    pub fn undo(&mut self) {
        let result = {
            let catalog = ready(&self.catalogs, self.game());
            let Some(open) = &mut self.open else {
                return;
            };
            open.session.undo(&Ground::of(catalog))
        };
        self.after(result);
    }

    fn after(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.note = None;
                self.refresh_members();
            }
            Err(why) => self.note = Some(why),
        }
    }

    /// The party strip's pick.
    pub fn member(&self) -> Option<&Member> {
        self.open.as_ref()?.members.get(self.member)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let busy = self
            .catalogs
            .values()
            .any(|loading| matches!(loading, Loading::Pending(_)))
            || self.open.as_ref().is_some_and(|open| open.level.is_some());
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let start = std::time::Instant::now();
        self.draw(ui);
        self.shot.drawn(start.elapsed(), busy);
        self.shot.frame(ui.ctx(), busy);
    }
}

/// `game`'s names, when read.
fn ready(catalogs: &HashMap<Game, Loading>, game: Option<Game>) -> Option<&Catalog> {
    match catalogs.get(&game?)? {
        Loading::Ready(Ok(catalog)) => Some(catalog),
        _ => None,
    }
}
