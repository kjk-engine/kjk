//! The presentation and flow routines (I35, `docs/SCRIPTS.md`), as TSL's
//! executable runs them, and K1's where it has them: time conversions,
//! movie queues, clicks held off, barks queued, feedback floating over
//! objects, tutorial windows, the map jammed and its return button, bodies
//! changed for good, and the small ones that change nothing here.

use script_ncs::vm::{ObjectId, Value};

use super::actions::Action;
use super::engine::{ANIMATION_MS, Bound, Engine, declared, float, int, object, text};
use super::state::{global, set_global};
use super::{Change, State, key};
use crate::module::{Global, ModuleData};
use crate::notice::{Notice, Screen};
use crate::snapshot::{Screens, Transit};

/// The script KOTOR runs each time an in-game screen opens (both games'
/// executables, `ShowSWInGameGui`): it sets the map page's return button.
pub const SCREEN_OPENED: &str = "k_sup_guiopen";

/// The script TSL's executable runs as the player closes the status
/// summary (`CSWGuiStatusSummary::HandleInputEvent`): it plays the Visas
/// Marr movies once the player's alignment has gone far enough, and so
/// sends Visas to the Ebon Hawk (`003EBO_VISAS_ARRIVES`). K1 has none.
pub const SUMMARY_CLOSED: &str = "k_align_movie";

/// The scripts the engine runs of its own accord, which a module's library
/// gathers whoever names them.
pub const ENGINE_SCRIPTS: [&str; 2] = [SCREEN_OPENED, SUMMARY_CLOSED];

/// A combat round and a turn, in seconds, as both games' executables count
/// them (`TimeConversions`, not `nwscript.nss`'s; a turn in rounds).
const ROUND_S: f32 = rules::dialogue::clock::ROUND_S;
const ROUNDS_A_TURN: f32 = rules::dialogue::clock::ROUNDS_A_TURN;

/// `difficultyopt.2da`'s Normal row, what play here matches until a
/// difficulty option exists (`GetGameDifficulty`).
const NORMAL: i32 = 1;

const GAME_COMPLETE: &str = "GBL_GAME_COMPLETE";

/// The one animation `PlayOverlayAnimation` plays (its executable acts on
/// no other), `ANIMATION_FIREFORGET_DIVE_ROLL`: the overlay id it maps to is
/// `animations.2da`'s `diveroll`.
const OVERLAY: u16 = 123;

/// The local boolean `DisplayDatapad` sets on the item shown.
const DATAPAD_READ: i32 = 94;

/// The `tutorial.2da` rows K1's `ShowTutorialWindow` shows for its window
/// numbers 0, 1 and 2 (its executable's own table): the swoop race, return
/// to base, the movement keys.
const K1_WINDOWS: [u8; 3] = [9, 40, 42];

/// TSL's `NPC_BAO_DUR`: met once his influence is set (`Screens::bao_dur_met`).
const BAO_DUR: u8 = 1;

/// The highest input class `SetInputClass` takes (0 PC control to 5).
const INPUT_CLASSES: i32 = 5;

/// `n` rounds, hours or turns in seconds (`RoundsToSeconds`,
/// `HoursToSeconds`, `TurnsToSeconds`): an hour lasts the IFO's minutes of
/// sixty seconds (`rules::scripts::clock::MINUTES_PER_HOUR` where it does
/// not say, as the clock reads it).
fn seconds(which: Bound, n: i32, minutes_per_hour: u8) -> f32 {
    let n = n as f32;
    match which {
        Bound::HoursToSeconds => {
            let minutes = if minutes_per_hour == 0 {
                rules::scripts::clock::MINUTES_PER_HOUR
            } else {
                minutes_per_hour
            };
            n * 60.0 * f32::from(minutes)
        }
        Bound::TurnsToSeconds => n * ROUND_S * ROUNDS_A_TURN,
        _ => n * ROUND_S,
    }
}

impl State {
    /// The in-game screens as the scripts leave them (`snapshot::Screens`)
    /// in `module`: the party table's map, the return button as
    /// `SetReturnStrref` set it, the area's transit as a script set it,
    /// else its ARE's, and Bao-Dur met (his influence set, as the
    /// executable's item descriptions read it).
    pub fn screens(&self, module: Option<&ModuleData>) -> Screens {
        let (show, label, query) = self.return_button;
        let disabled = module.is_some_and(|module| {
            (self.transit.get(&key(&module.name)).copied()).unwrap_or(module.disable_transit)
        });
        Screens {
            map_disabled: self.party.no_map,
            bao_dur_met: self.party.influence(BAO_DUR) != -1,
            transit: Transit {
                show,
                label,
                query,
                disabled,
            },
        }
    }
}

impl super::Step<'_> {
    /// KOTOR's status summary, as its client shows it: the step's statuses
    /// owe one, which shows once no conversation holds the world (the
    /// client gathers them meanwhile); the player then closes it, and TSL's
    /// executable runs `SUMMARY_CLOSED` (taken as closed at once here: no
    /// player waits on it headless [verify in the exe]).
    pub(super) fn summary_closed(&mut self) {
        self.state.summary_owed |= !self.out.statuses.is_empty();
        let talking = self.state.talk.holds_world(&self.view.seat_bodies());
        if !self.state.summary_owed || talking || self.out.new_module.is_some() {
            return;
        }
        self.state.summary_owed = false;
        if self.library.program(SUMMARY_CLOSED).is_some() {
            let closed = super::Pending::new(SUMMARY_CLOSED, super::MODULE);
            self.state.pending.push(closed);
            self.run_pending();
        }
    }
}

impl Engine<'_> {
    /// The presentation and flow routines (the module documentation).
    pub(super) fn presentation_call(
        &mut self,
        bound: Bound,
        caller: ObjectId,
        args: &[Value],
    ) -> Value {
        use Bound::*;
        let view = self.view;
        match bound {
            RoundsToSeconds | HoursToSeconds | TurnsToSeconds => {
                Value::Float(seconds(bound, int(args, 0), view.module.minutes_per_hour))
            }
            // Degrees to a unit vector across the floor.
            AngleToVector => {
                let (sin, cos) = crate::math::sin_cos_deg(float(args, 0));
                Value::Vector([cos, sin, 0.0])
            }
            GetGameDifficulty => Value::Int(NORMAL),
            // Both games' movies hold the script until they end, so a script
            // never sees one playing.
            IsMoviePlaying => Value::Int(0),
            QueueMovie => {
                self.queue_movie(text(args, 0), int(args, 1) != 0);
                Value::Int(0)
            }
            PlayMovieQueue => self.play_movie_queue(),
            // KOTOR's input classes give control back after a minigame or a
            // cutscene; here the sim's own modes hold input, so it changes
            // nothing (0 to 5 taken, the rest ignored, as the executable).
            SetInputClass => {
                let class = int(args, 0);
                if (0..=INPUT_CLASSES).contains(&class) {
                    self.trace(&format!("  input class {class}"));
                }
                Value::Int(0)
            }
            // No title jukebox to unlock songs for; a pause is each player's
            // own (a break in co-op is a player's call, never a script's);
            // no script calls the E3 demo's screen or the rain switch.
            UnlockAllSongs | PauseGame | EnableRain | ShowDemoScreen => Value::Int(0),
            // TSL's executable forgets the companion a conversation hands
            // control back to as it closes: one a companion's player began
            // with an object whose `PartyInteract` is off, which its player's
            // own character speaks instead. That hand-over is not made here
            // (a companion speaks for its player, `script::dialogue`), so
            // there is nothing to forget [verify the hand-over].
            CancelPostDialogCharacterSwitch => Value::Int(0),
            // The door shut at once (its closed state, no action, no event).
            YavinHackDoorClose => {
                let door = object(args, 0);
                if self.kind(door) == Some(crate::module::ObjectKind::Door) {
                    self.out.changes.push(Change::Door {
                        door,
                        open: false,
                        by: door,
                    });
                }
                Value::Int(0)
            }
            SoundObjectGetPitchVariance => Value::Float(
                self.instance(object(args, 0))
                    .filter(|facts| facts.kind == crate::module::ObjectKind::Sound)
                    .map_or(0.0, |facts| facts.pitch_variation),
            ),
            SoundObjectSetPosition => {
                let object = object(args, 0);
                if self.kind(object) == Some(crate::module::ObjectKind::Sound) {
                    let to = super::engine::vector(args, 1);
                    self.out.changes.push(Change::Place { object, to });
                }
                Value::Int(0)
            }
            // Game time, a pause not counting; a later call starts afresh.
            NoClicksFor => {
                let seconds = float(args, 0);
                if seconds > 0.0 {
                    let ms = (seconds * 1000.0) as u64;
                    self.state.no_clicks_until = view.now_ms + ms;
                }
                Value::Int(0)
            }
            // `BarkString` of the caller, queued: nothing while a
            // conversation holds the world, nothing for what is not a
            // creature.
            ActionBarkString => {
                let held = self.state.talk.holds_world(&view.seat_bodies());
                if let Ok(strref) = u32::try_from(int(args, 0))
                    && !held
                    && self.character(caller).is_some()
                {
                    self.queue(caller, Action::Bark { strref });
                }
                Value::Int(0)
            }
            SetKeepStealthInDialog => {
                self.state.keep_stealth = int(args, 0) != 0;
                Value::Int(0)
            }
            DisplayFeedBackText => {
                let over = object(args, 0);
                if view.thing(over).is_some() {
                    let tables = self.library.presentation();
                    let strref = usize::try_from(int(args, 1))
                        .ok()
                        .and_then(|row| tables.feedback.get(row).copied())
                        .unwrap_or(0);
                    self.out.feedback.push((over, strref));
                }
                Value::Int(0)
            }
            // TSL's takes the row as a byte; K1's a window number, 0 to 2,
            // for its rows (the rest show nothing).
            ShowTutorialWindow => {
                let n = int(args, 0);
                let row = match view.module.game.as_str() {
                    "k1" => (usize::try_from(n).ok()).and_then(|n| K1_WINDOWS.get(n).copied()),
                    _ => Some(n as u8),
                };
                row.map_or(Value::Int(0), |row| self.tutorial(row))
            }
            DisableMap => {
                self.state.party.no_map = int(args, 0) != 0;
                Value::Int(0)
            }
            SetDisableTransit => {
                let module = key(&view.module.name);
                self.state.transit.insert(module, int(args, 0) != 0);
                Value::Int(0)
            }
            SetReturnStrref => {
                let strref = |at| u32::try_from(int(args, at)).unwrap_or(0);
                self.state.return_button = (int(args, 0) != 0, strref(1), strref(2));
                Value::Int(0)
            }
            ChangeObjectAppearance => self.change_appearance(object(args, 0), int(args, 1)),
            DuplicateHeadAppearance => self.duplicate_head(object(args, 0), object(args, 1)),
            // Only the dive roll, at once, outside the queue, on a creature.
            PlayOverlayAnimation => {
                let who = object(args, 0);
                if int(args, 1) == i32::from(OVERLAY) && self.character(who).is_some() {
                    let clip = (view.appearance(who))
                        .and_then(|row| self.library.clip_ms(row, OVERLAY))
                        .unwrap_or(ANIMATION_MS);
                    self.out.changes.push(Change::Animate {
                        who,
                        play: (OVERLAY, clip),
                    });
                }
                Value::Int(0)
            }
            DisplayDatapad => {
                let item = object(args, 0);
                let template = self.state.items.get(&item.0).map(|i| i.template.clone());
                match template {
                    Some(template) => {
                        self.state.locals.insert((item.0, 0, DATAPAD_READ), 1);
                        self.tell_all(Notice::Show(Screen::Datapad { template }))
                    }
                    None => Value::Int(0),
                }
            }
            _ => unreachable!("not a presentation routine: {bound:?}"),
        }
    }

    /// `QueueMovie`: the movie joins the queue with its own skip; one of
    /// TSL's endings counts the game complete.
    fn queue_movie(&mut self, movie: &str, skippable: bool) {
        if crate::notice::ENDING_MOVIES
            .iter()
            .any(|ending| ending.eq_ignore_ascii_case(movie))
            && declared(self.view.module, GAME_COMPLETE, Global::Number)
        {
            let count = global(&self.state.numbers, GAME_COMPLETE)
                .copied()
                .unwrap_or(0);
            let count = i32::from((count + 1) as i8);
            set_global(&mut self.state.numbers, GAME_COMPLETE, count);
        }
        self.state.movies.push((movie.to_string(), skippable));
    }

    /// `PlayMovieQueue`: the queue told to every player in order, one run
    /// (its `nAllowSkips` is kept and never read by the executable:
    /// QUIRKS), and emptied.
    fn play_movie_queue(&mut self) -> Value {
        let movies = std::mem::take(&mut self.state.movies);
        let run = self.state.movie_runs.wrapping_add(1).max(1);
        self.state.movie_runs = run;
        for (movie, skippable) in movies.iter().filter(|(movie, _)| !movie.is_empty()) {
            self.trace(&format!("  plays the queued movie {movie}"));
            self.tell_all(Notice::Show(Screen::queued_movie(movie, *skippable, run)));
        }
        Value::Int(0)
    }

    /// `ShowTutorialWindow(row)`: its window, once, where the row has words
    /// and no conversation holds the world; the row marked shown then
    /// (each player's "Tutorial Popups" option keeps it off its screen).
    fn tutorial(&mut self, row: u8) -> Value {
        let tables = self.library.presentation();
        let valid = tables.tutorials.get(usize::from(row)) == Some(&true);
        let talking = self.state.talk.holds_world(&self.view.seat_bodies());
        if !valid || talking || self.state.party.tutorials.shown(row) {
            return Value::Int(0);
        }
        self.state.party.tutorials.show(row);
        self.tell_all(Notice::Show(Screen::Tutorial { row }))
    }

    /// `ChangeObjectAppearance`: a creature not under a disguise takes the
    /// appearance for good and stands where its new body fits.
    fn change_appearance(&mut self, who: ObjectId, appearance: i32) -> Value {
        let (Some(c), Ok(appearance)) = (self.character(who), u16::try_from(appearance)) else {
            return Value::Int(0);
        };
        if c.sheet.gear.disguise.is_some() {
            self.trace(&format!(
                "  {}: disguised, its appearance stays",
                super::name(who)
            ));
            return Value::Int(0);
        }
        let look = crate::combat::Look {
            appearance: Some(appearance),
            ..c.look
        };
        self.out.changes.push(Change::Look { who, look });
        if let Some(thing) = self.view.thing(who) {
            let (to, yaw) = (thing.position, thing.yaw);
            self.out.changes.push(Change::Jump { who, to, yaw });
        }
        Value::Int(0)
    }

    /// `DuplicateHeadAppearance`: the creature takes the head of the other's
    /// appearance (`appearance.2da`'s `normalhead`).
    fn duplicate_head(&mut self, who: ObjectId, like: ObjectId) -> Value {
        let head = (self.view.appearance(like))
            .and_then(|row| {
                self.library
                    .presentation()
                    .heads
                    .get(usize::from(row))
                    .copied()
            })
            .and_then(|head| u8::try_from(head).ok());
        if let (Some(c), Some(head), true) =
            (self.character(who), head, self.character(like).is_some())
        {
            let look = crate::combat::Look {
                head: Some(head),
                ..c.look
            };
            self.out.changes.push(Change::Look { who, look });
        }
        Value::Int(0)
    }

    /// Something for every player.
    fn tell_all(&mut self, notice: Notice) -> Value {
        self.out.notices.push((None, notice));
        Value::Int(0)
    }

    /// An object's kind, if it is one of the area's.
    fn kind(&self, id: ObjectId) -> Option<crate::module::ObjectKind> {
        self.view.thing(id).and_then(|thing| thing.kind)
    }
}
