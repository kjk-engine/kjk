//! A character with its blade lit stands and walks in its own stance and
//! walk from a mod's skeleton (Movie Duels' per-character sequences,
//! `docs/SABER-RULES.md` section 11), in place of JA's (`game.duel_stances`,
//! off by default).
//!
//! - **What:** while the sim's skeleton holds one of the ready stances or
//!   walks a lit blade plays (`SWAPPED`), the drawn body plays the
//!   character's own sequence instead: the first its row finds of its named
//!   sequence and JA's name with each of its suffixes
//!   (`BOTH_SABERDUAL_STANCE` + `_MAUL`), looked up in the row's
//!   `skeleton` (Movie Duels' `_humanoid_kotor`: a skeleton of its own
//!   beside `_humanoid`, the same names played its way) or else in the
//!   stack's own `animation.cfg`. A skeleton's frames are added after the
//!   view's skeleton's own (`Borrowed`), when its bones are the same. The
//!   table is `data/duel_stances.toml`: rows by template or tag (`*` a
//!   wildcard), the player's own character, and a default row.
//! - **How:** the sim's playback of JA's sequence is replayed over the
//!   other one's frames from the same moment, at the same share of its
//!   own rate (a walk sped up by the pace stays sped up); a part going to
//!   or from such a sequence blends from the pose it shows (`Figure`).
//! - **Picks:** with the option on, each party member may stand in any of
//!   Movie Duels' skeletons instead of its row's, picked on the character
//!   page (`saber.stance.<who>`, `PICKED`; `headgear::who` names whom).
//!   A skeleton picked in play is read then (`DuelStances::repick`), its
//!   frames added to a copy of the view's skeleton every figure moves to
//!   (`Cast::repick`); each is read once a run.
//! - **A held block** (`docs/SABER-RULES.md` 11.5): moving, the parry
//!   toward where it moves (`Quadrant`: back or forward left `TL`/`BL`,
//!   right `TR`/`BR`, straight `T_`, sideways `TL`/`TR`), the legs walking
//!   on; standing, with attack the top parry, else the character's own
//!   standing guard (`BOTH_STAND_BLOCKING_ON`) from its Movie Duels
//!   skeleton, with the stances on or off, else the top parry. Two blades
//!   play `P6_S6`, a staff `P7_S7`.
//! - **A perfect parry** (`docs/SABER-RULES.md` 3.1): with one blade, a
//!   blow met at the top left or the top right draws Movie Duels' own
//!   knockaway (`PERFECT`) over the sim's, from the character's Movie
//!   Duels skeleton, with the stances on or off. A blow met anywhere else,
//!   two blades and a staff draw the sim's knockaway.
//! - **Without it** (the option off, no such sequence in the stack, no
//!   blade lit, a body drawn the cgame's way): JA's sequence plays.
//!
//! Render only, as `stance`: the sim's skeleton, which the blades cut
//! with, holds JA's sequence, so no digest moves.

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::Gla;
use sim::combat::saber::blades::Shown;
use sim::motion::Anim;
use sim::skeleton::Playback;
use sim::skeleton::stance::glob;
use sim::{EntityState, Instances, Kind};

/// The setting that turns them on (`true`).
pub const SETTING: &str = "game.duel_stances";

use crate::stance_sets::{Lender, Row, TABLE, offered, rows};
pub use crate::stance_sets::{offered_folders, pick_setting, picks, title};

/// The sim's sequences a lit blade stands, fidgets or walks in
/// (`sim::motion::weapon`, `sim::combat::idle`): the ones a character's
/// own may stand for. The fidgets go with the stance, or a character in
/// another's stance would snap back to JA's now and then.
pub const SWAPPED: [Anim; 10] = [
    Anim::BOTH_STAND2,
    Anim::BOTH_STAND2IDLE1,
    Anim::BOTH_STAND2IDLE2,
    Anim::BOTH_SABERFAST_STANCE,
    Anim::BOTH_SABERSLOW_STANCE,
    Anim::BOTH_SABERDUAL_STANCE,
    Anim::BOTH_SABERSTAFF_STANCE,
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
];

/// A held block's standing guard (`docs/SABER-RULES.md` 11.5), for one
/// blade, two and a staff: Movie Duels' skeletons have it, JA's not.
const STANDING: [&str; 3] = [
    "BOTH_STAND_BLOCKING_ON",
    "BOTH_STAND_BLOCKING_ON_DUAL",
    "BOTH_STAND_BLOCKING_ON_STAFF",
];

/// A held block's parries, for one blade, two and a staff, each followed
/// by its quadrant (`QUADRANTS`): JA's own and Movie Duels'.
const PARRIES: [&str; 3] = ["BOTH_P1_S1_", "BOTH_P6_S6_", "BOTH_P7_S7_"];

/// The parries' quadrants, in `Quadrant`'s order.
const QUADRANTS: [&str; 5] = ["T_", "TL", "TR", "BL", "BR"];

/// The endings of a standing guard's name (`STANDING`) that give its
/// guard while its holder moves, in `Step`'s order. Movie Duels' skeletons
/// have all four for each of `STANDING`. JA's skeleton has none.
const STEPS: [&str; 4] = ["_FORWARD", "_BACK", "_LEFT", "_RIGHT"];

/// Which way the holder of a held block moves, for the guard it shows
/// while it moves (`STEPS`). A move with a sideways part counts as a move
/// to that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Forward,
    Back,
    Left,
    Right,
}

impl Step {
    /// The step of a command's `forward` and `right`. Standing still gives
    /// none.
    pub fn of(forward: i8, right: i8) -> Option<Step> {
        Some(match (forward.signum(), right.signum()) {
            (_, -1) => Step::Left,
            (_, 1) => Step::Right,
            (1, _) => Step::Forward,
            (-1, _) => Step::Back,
            _ => return None,
        })
    }
}

/// Movie Duels' perfect parries for one blade, each with JA's knockaway it
/// is drawn over: a blow met at the top left, and one met at the top right.
const PERFECT: [(Anim, &str); 2] = [
    (Anim::BOTH_K1_S1_TL, "BOTH_K1_S1_TL_MD"),
    (Anim::BOTH_K1_S1_TR, "BOTH_K1_S1_TR_MD"),
];

/// Where a held block's parry faces (`QUADRANTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quadrant {
    Top,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Quadrant {
    /// The parry a held block shows while its holder moves by a command's
    /// `forward` and `right` (Movie Duels' directional blocking): back and
    /// to a side, or to a side alone, the top left or right; forward and to
    /// a side the bottom left or right; straight back or forward the top.
    /// None standing still.
    pub fn of(forward: i8, right: i8) -> Option<Quadrant> {
        Some(match (forward.signum(), right.signum()) {
            (0, 0) => return None,
            (1, -1) => Quadrant::BottomLeft,
            (1, 1) => Quadrant::BottomRight,
            (_, -1) => Quadrant::TopLeft,
            (_, 1) => Quadrant::TopRight,
            _ => Quadrant::Top,
        })
    }
}

/// A held block's poses (`STANDING`, `STEPS`, `PARRIES`), by blades, and
/// one blade's perfect parries (`PERFECT`).
#[derive(Debug, Clone, Default, PartialEq)]
struct Guards {
    standing: [Option<Sequence>; 3],
    moving: [[Option<Sequence>; 4]; 3],
    parries: [[Option<Sequence>; 5]; 3],
    perfect: [Option<Sequence>; 2],
}

impl Guards {
    fn any(&self) -> bool {
        self.standing
            .iter()
            .chain(self.moving.iter().flatten())
            .chain(self.parries.iter().flatten())
            .chain(&self.perfect)
            .any(Option::is_some)
    }

    /// Drops the guards for two blades and the staff that are the one-blade
    /// guard's. When the standing guard for two blades or the staff plays
    /// the one-blade standing guard's frames, all of that weapon's guards,
    /// standing and moving, are dropped. A moving guard that plays the
    /// one-blade guard's frames for the same step, or its standing guard's,
    /// is dropped on its own. The lookup then falls back to the stack's
    /// guard for that weapon (`DuelStances::pose`).
    fn own_weapons(&mut self) {
        let [single, rest @ ..] = &mut self.standing;
        let [single_moving, moving @ ..] = &mut self.moving;
        let one_blade = |sequence: &Sequence, step: Option<usize>| {
            let same =
                |other: &Option<Sequence>| other.as_ref().is_some_and(|o| same_frames(o, sequence));
            same(single) || step.is_some_and(|step| same(&single_moving[step]))
        };
        for (standing, moving) in rest.iter_mut().zip(moving) {
            if standing.as_ref().is_some_and(|s| one_blade(s, None)) {
                *standing = None;
                *moving = Default::default();
            }
            for (step, guard) in moving.iter_mut().enumerate() {
                if guard.as_ref().is_some_and(|g| one_blade(g, Some(step))) {
                    *guard = None;
                }
            }
        }
    }
}

/// `STANDING`'s, `STEPS`', `PARRIES`' and `PERFECT`'s sequences as `find`
/// finds them.
///
/// A guard for two blades or the staff that plays the one-blade guard's
/// frames is a one-handed pose with the wrong weapon in the hand, so it
/// counts as missing (`Guards::own_weapons`).
fn guards(mut find: impl FnMut(&str) -> Option<Sequence>) -> Guards {
    let mut guards = Guards {
        standing: STANDING.map(&mut find),
        moving: STANDING.map(|standing| STEPS.map(|step| find(&format!("{standing}{step}")))),
        parries: PARRIES.map(|parry| QUADRANTS.map(|quadrant| find(&format!("{parry}{quadrant}")))),
        perfect: PERFECT.map(|(_, name)| find(name)),
    };
    guards.own_weapons();
    guards
}

/// Whether `a` and `b` play the same frames.
fn same_frames(a: &Sequence, b: &Sequence) -> bool {
    (a.first_frame, a.frame_count) == (b.first_frame, b.frame_count)
}

/// A row as the stack's skeleton has it: each of JA's sequences it draws
/// otherwise, and the sequence drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub name: String,
    pub swaps: Vec<(Anim, Sequence)>,
    /// Its held block's poses (`STANDING`, `PARRIES`).
    guards: Guards,
}

/// Every row with a sequence in the stack, and whom each is for; the
/// skeletons offered and picked.
#[derive(Default)]
pub struct DuelStances {
    pub looks: Vec<Look>,
    /// (templates, tags, for the player) by look; none for a pick's.
    who: Vec<(Vec<String>, Vec<String>, bool)>,
    default: Option<u16>,
    /// The stack's own held block's poses, for those without a look's.
    guards: Guards,
    /// The looks draw their stances and walks (`game.duel_stances`); off,
    /// only their held block's poses.
    swaps: bool,
    /// The skeletons a character may be given (`offered`), by folder.
    pub offered: Vec<String>,
    /// The picks made (`picks`), by `who`; none while `swaps` is off.
    picks: BTreeMap<String, String>,
    /// Each skeleton picked so far, read once: its look, if it has one.
    picked: BTreeMap<String, Option<u16>>,
    /// Where a skeleton picked in play is read from.
    files: Option<Arc<crate::viewer_ja::Files<'static>>>,
}

impl DuelStances {
    /// Without Movie Duels: no looks, the held block's poses the stack's
    /// (`sequences`, by upper-case name).
    pub fn plain(sequences: &BTreeMap<String, Sequence>) -> DuelStances {
        DuelStances {
            guards: guards(|name| sequences.get(name).cloned()),
            ..DuelStances::default()
        }
    }

    /// Whether `settings` turn the stances on, with Movie Duels installed
    /// (`settings::movie_duels`): its characters' skeletons are its own.
    pub fn wanted(settings: &BTreeMap<String, String>) -> bool {
        settings.get(SETTING).is_some_and(|value| value == "true") && crate::settings::movie_duels()
    }

    /// The shipped table and `settings`' picks against the stack's files
    /// (`stack`, Movie Duels' among them; `sequences` its `animation.cfg`'s,
    /// by upper-case name, played on `gla`), the stances and walks drawn
    /// only with `swaps`, with what it found said in `notes`; and `gla`
    /// with the frames the skeletons lend it, when they lend any.
    pub fn load(
        stack: &'static vfs::ja::JaStack,
        (gla, sequences): (&Gla, &BTreeMap<String, Sequence>),
        swaps: bool,
        settings: &BTreeMap<String, String>,
        notes: &mut Vec<String>,
    ) -> (DuelStances, Option<Gla>) {
        let rows = match rows(TABLE) {
            Ok(rows) => rows,
            Err(error) => {
                notes.push(format!("data/duel_stances.toml: {error}"));
                return (DuelStances::plain(sequences), None);
            }
        };
        let files = Arc::new(crate::viewer_ja::Files::new(stack));
        let mut borrowed = Borrowed::new(gla);
        let mut stances = DuelStances {
            swaps,
            offered: offered(&files),
            ..DuelStances::plain(sequences)
        };
        stances.of(
            &rows,
            |row, name| match &row.skeleton {
                Some(skeleton) => borrowed.take(&files, skeleton, name),
                None => sequences.get(name).cloned(),
            },
            notes,
        );
        stances.files = Some(files);
        notes.append(&mut borrowed.notes);
        let lent = borrowed.extended;
        let grown = stances.repick(picks(settings), lent.as_ref().unwrap_or(gla), notes);
        (stances, grown.flatten().or(lent))
    }

    /// `rows`, each sequence looked up by `find` (a row, a name in upper
    /// case): a row none of whose sequences is found is left out.
    fn of(
        &mut self,
        rows: &[Row],
        mut find: impl FnMut(&Row, &str) -> Option<Sequence>,
        notes: &mut Vec<String>,
    ) {
        for row in rows {
            let Some(look) = self.add(row, |name| find(row, name), notes) else {
                continue;
            };
            let default = row.templates.is_empty() && row.tags.is_empty() && !row.player;
            if default && self.default.is_none() {
                self.default = Some(look);
            }
            self.who[usize::from(look)] = (row.templates.clone(), row.tags.clone(), row.player);
        }
    }

    /// `row`'s look, each sequence looked up by `find` (a name in upper
    /// case): its stances and walks (with `swaps`) and its held block's
    /// poses; none when none is found.
    fn add(
        &mut self,
        row: &Row,
        mut find: impl FnMut(&str) -> Option<Sequence>,
        notes: &mut Vec<String>,
    ) -> Option<u16> {
        let swaps: Vec<(Anim, Sequence)> = SWAPPED
            .iter()
            .filter(|_| self.swaps)
            .filter_map(|&anim| {
                let own = anim.name();
                // In a skeleton of its own, JA's name is its own look.
                let plain = row.skeleton.is_some().then(|| own.to_string());
                let named = row.sequences.get(own).cloned();
                let suffixed = row.suffixes.iter().map(|suffix| format!("{own}{suffix}"));
                let found = named
                    .into_iter()
                    .chain(suffixed)
                    .chain(plain)
                    .find_map(|name| find(&name.to_ascii_uppercase()))?;
                Some((anim, found))
            })
            .collect();
        let guards = guards(&mut find);
        let shown: Vec<String> = swaps
            .iter()
            .map(|(anim, sequence)| format!("{} as {}", anim.name(), sequence.name))
            .collect();
        let found = match (shown.is_empty(), guards.any()) {
            (false, _) => shown.join(", "),
            (true, true) => "a held block's poses".to_string(),
            (true, false) => "none in the stack".to_string(),
        };
        notes.push(format!(
            "duel stances, {}{}: {found}",
            row.name,
            row.skeleton
                .as_ref()
                .map_or(String::new(), |skeleton| format!(" ({skeleton})")),
        ));
        if swaps.is_empty() && !guards.any() {
            return None;
        }
        let look = self.looks.len() as u16;
        self.looks.push(Look {
            name: row.name.clone(),
            swaps,
            guards,
        });
        self.who.push((Vec::new(), Vec::new(), false));
        Some(look)
    }

    /// The picks as `picks` (`picks`, by `who`) now are, while the stances
    /// are on (off, none count): each skeleton not picked before read,
    /// its frames lent to `gla` (the view's skeleton). None when they are as
    /// they were; else the view's skeleton with what the new ones lend, if
    /// they lend any.
    pub fn repick(
        &mut self,
        picks: BTreeMap<String, String>,
        gla: &Gla,
        notes: &mut Vec<String>,
    ) -> Option<Option<Gla>> {
        let files = self.files.clone();
        let mut borrowed = Borrowed::new(gla);
        let changed = self.pick(
            picks,
            |folder, name| borrowed.take(files.as_deref()?, folder, name),
            notes,
        );
        notes.append(&mut borrowed.notes);
        changed.then_some(borrowed.extended)
    }

    /// `picks` taken (none while the stances are off), each skeleton not
    /// picked before looked up by `find` (a folder, a name in upper case):
    /// whether they changed.
    fn pick(
        &mut self,
        picks: BTreeMap<String, String>,
        mut find: impl FnMut(&str, &str) -> Option<Sequence>,
        notes: &mut Vec<String>,
    ) -> bool {
        let picks = if self.swaps { picks } else { BTreeMap::new() };
        if picks == self.picks {
            return false;
        }
        for folder in picks.values() {
            if self.picked.contains_key(folder) {
                continue;
            }
            let row = Row {
                name: title(folder),
                skeleton: Some(folder.clone()),
                ..Row::default()
            };
            let look = self.add(&row, |name| find(folder, name), notes);
            self.picked.insert(folder.clone(), look);
        }
        self.picks = picks;
        true
    }

    /// The look `entity` stands in: its pick's (`headgear::who`), else its
    /// template's or tag's row, the player's for a seat's own character,
    /// else the default row's.
    pub fn look_of(&self, entity: &EntityState, objects: &Instances) -> Option<u16> {
        let data = objects.of_entity(entity);
        let who = crate::headgear::who(Some(objects), entity);
        self.look_for(
            who.as_deref(),
            data.map(|data| (data.template.as_str(), data.tag.as_str())),
            matches!(entity.kind, Kind::Avatar(_)),
        )
    }

    /// `look_of` for a character named `who`, of (template, tag) `data`,
    /// or a seat's (`player`).
    fn look_for(&self, who: Option<&str>, data: Option<(&str, &str)>, player: bool) -> Option<u16> {
        let picked = who
            .and_then(|who| self.picks.get(who))
            .and_then(|folder| *self.picked.get(folder)?);
        if picked.is_some() {
            return picked;
        }
        let found = self.who.iter().position(|(templates, tags, for_player)| {
            (player && *for_player)
                || data.is_some_and(|(template, tag)| {
                    templates.iter().any(|t| glob(t, template)) || tags.iter().any(|t| glob(t, tag))
                })
        });
        found.map(|look| look as u16).or(self.default)
    }

    /// The pose a held block shows over `anim` in `look` (the look's own
    /// first, else the stack's), by the blades `anim` holds, while its
    /// holder moves by a command's `forward` and `right` (`way`).
    ///
    /// With attack held (`aimed`), the pose is the parry toward the move
    /// (`Quadrant::of`), and the top parry while the holder stands. A staff
    /// held standing still shows its standing guard instead of the top
    /// parry. Without
    /// attack, a moving holder shows the standing guard for its step
    /// (`Step::of`), as Movie Duels' block does while its holder moves. A
    /// holder that stands still shows the standing guard. Where the guard
    /// for a step is missing, the standing guard shows. Where that is
    /// missing too, the parry toward the move shows, and the top parry last.
    pub fn guard(
        &self,
        look: Option<u16>,
        anim: Anim,
        aimed: bool,
        (forward, right): (i8, i8),
    ) -> Option<&Sequence> {
        let blades = sim::skeleton::stance::blades(anim);
        let own = self.own(look);
        let quadrant = Quadrant::of(forward, right);
        let guarded = || {
            Step::of(forward, right)
                .and_then(|step| self.pose(own, |g| g.moving[blades][step as usize].as_ref()))
                .or_else(|| self.pose(own, |g| g.standing[blades].as_ref()))
        };
        let parry =
            |quadrant: Quadrant| self.pose(own, |g| g.parries[blades][quadrant as usize].as_ref());
        // JA's top parry with the staff holds the staff level through a
        // KOTOR body's face, so an aimed staff block standing still shows
        // the standing guard.
        let staff_top = aimed && blades == 2 && quadrant.is_none();
        (!aimed || staff_top)
            .then(guarded)
            .flatten()
            .or_else(|| quadrant.and_then(parry))
            .or_else(|| parry(Quadrant::Top))
    }

    /// The pose `of` picks from a held block's poses, `own` (a look's)
    /// first, then the default row's look's, else the stack's.
    fn pose<'s>(
        &'s self,
        own: Option<&'s Guards>,
        of: impl Fn(&'s Guards) -> Option<&'s Sequence>,
    ) -> Option<&'s Sequence> {
        own.and_then(&of)
            .or_else(|| self.own(self.default).and_then(&of))
            .or_else(|| of(&self.guards))
    }

    /// The sequence a perfect parry shows over `anim` in `look` (the look's
    /// own first, else the stack's). JA's knockaway to the top left or the
    /// top right with one blade shows Movie Duels' own (`PERFECT`). Every
    /// other knockaway shows none, and JA's plays on.
    pub fn perfect<'s>(&'s self, look: Option<u16>, anim: Anim) -> Option<&'s Sequence> {
        let side = PERFECT.iter().position(|(over, _)| *over == anim)?;
        let of = |guards: &'s Guards| guards.perfect[side].as_ref();
        self.pose(self.own(look), of)
    }

    /// The held block's poses and perfect parries of `look`'s own.
    fn own(&self, look: Option<u16>) -> Option<&Guards> {
        let look = self.looks.get(usize::from(look?))?;
        Some(&look.guards)
    }

    /// Whether `anim` is a walk or a run, which the legs keep under a held
    /// block.
    pub fn moves(anim: Anim) -> bool {
        sim::skeleton::stance::moves(anim)
    }

    /// Whether a held block's pose shows over `anim`: a lit blade's ready
    /// stance, fidget, walk or run, or its lowering to rest and back (the
    /// sim's idle blade lowers while block is held, swinging nothing).
    pub fn guarded(anim: Anim) -> bool {
        SWAPPED.contains(&anim)
            || Self::moves(anim)
            || matches!(anim, Anim::BOTH_STAND2TO1 | Anim::BOTH_STAND1TO2)
    }

    /// The sequence `look` draws for `anim`, if it draws another.
    pub fn swap(&self, look: Option<u16>, anim: Anim) -> Option<&Sequence> {
        let look = self.looks.get(usize::from(look?))?;
        look.swaps
            .iter()
            .find(|(swapped, _)| *swapped == anim)
            .map(|(_, sequence)| sequence)
    }
}

/// The skeletons the rows look their sequences up in, each read once, and
/// the view's skeleton with the frames taken from them added after its
/// own (`extended`): a sequence taken is its frames there.
struct Borrowed<'g> {
    gla: &'g Gla,
    extended: Option<Gla>,
    /// By folder: its frames and `animation.cfg`, or none (said once).
    read: BTreeMap<String, Option<Lender>>,
    /// (folder, name) to the frames taken.
    taken: BTreeMap<(String, String), Option<Sequence>>,
    /// (folder, first frame, frame count) in the folder's skeleton to the
    /// first frame where they were added: two names for the same frames
    /// share them.
    added: BTreeMap<(String, i32, i32), i32>,
    notes: Vec<String>,
}

impl<'g> Borrowed<'g> {
    fn new(gla: &'g Gla) -> Borrowed<'g> {
        Borrowed {
            gla,
            extended: None,
            read: BTreeMap::new(),
            taken: BTreeMap::new(),
            added: BTreeMap::new(),
            notes: Vec::new(),
        }
    }

    /// `skeleton`'s sequence `name` (upper case), its frames added to the
    /// view's skeleton.
    fn take(
        &mut self,
        files: &crate::viewer_ja::Files,
        skeleton: &str,
        name: &str,
    ) -> Option<Sequence> {
        let key = (skeleton.to_ascii_lowercase(), name.to_string());
        if let Some(taken) = self.taken.get(&key) {
            return taken.clone();
        }
        let (gla, notes) = (self.gla, &mut self.notes);
        let lent = self
            .read
            .entry(key.0.clone())
            .or_insert_with(|| {
                crate::stance_sets::read_lender(files, &key.0)
                    .and_then(
                        |lent| match crate::stance_sets::other_bones(&lent.gla, gla) {
                            Some(why) => Err(why),
                            None => Ok(lent),
                        },
                    )
                    .inspect_err(|error| notes.push(format!("duel stances, {}: {error}", key.0)))
                    .ok()
            })
            .as_ref();
        let added = &mut self.added;
        let taken = lent.and_then(|lent| {
            let (from, sequence) = (&lent.gla, lent.sequences.get(name)?);
            let frames = (key.0.clone(), sequence.first_frame, sequence.frame_count);
            if let Some(&first_frame) = added.get(&frames) {
                return Some(Sequence {
                    first_frame,
                    ..sequence.clone()
                });
            }
            let into = self.extended.get_or_insert_with(|| gla.clone());
            let appended = append(into, from, sequence)?;
            added.insert(frames, appended.first_frame);
            Some(appended)
        });
        self.taken.insert(key, taken.clone());
        taken
    }
}

/// `sequence`'s frames of `from` added after `into`'s own (the bone
/// transforms they use with them): the sequence where they now are.
fn append(into: &mut Gla, from: &Gla, sequence: &Sequence) -> Option<Sequence> {
    let bones = into.bones.len();
    let first = usize::try_from(sequence.first_frame).ok()?;
    let count = usize::try_from(sequence.frame_count).ok()?;
    let frames = from.frames.get(first * bones..(first + count) * bones)?;
    let mut moved = BTreeMap::new();
    for &index in frames {
        let pool = into.pool.len() as u32;
        let at = *moved.entry(index).or_insert_with(|| {
            into.pool.push(from.pool[index as usize]);
            pool
        });
        into.frames.push(at);
    }
    let start = into.frame_count;
    into.frame_count += count;
    Some(Sequence {
        first_frame: i32::try_from(start).ok()?,
        ..sequence.clone()
    })
}

/// `shown` (the sim's playback of `own`) replayed over `other`'s frames
/// from the same moment, at the same share of its own rate. Its blend is
/// the sim's, whose frames are what the sim played before: the caller
/// takes only its length, blending from the pose it shows.
pub fn replayed(shown: Shown, own: Option<&Sequence>, other: &Sequence) -> Shown {
    let rate = |sequence: &Sequence| crate::stage::sim_sequence(sequence).frame_lerp;
    let scale = match own.map(rate) {
        Some(lerp) if lerp != 0 && shown.playback.speed != 0.0 => {
            shown.playback.speed * lerp as f32 / 50.0
        }
        _ => 1.0,
    };
    let playback = Playback::set(
        None,
        &crate::stage::sim_sequence(other),
        scale,
        shown.playback.start_ms,
        0,
        None,
    );
    Shown {
        playback: Playback {
            blend: shown.playback.blend,
            ..playback
        },
        ..shown
    }
}

/// The duel stances' checks, without an install (`kjk sim probes`, group
/// "duel stances"): the table reads, a held block finds its blades'
/// guard and parries toward its move, a pick stands its character in the
/// skeleton picked (each read once), a skeleton's frames are lent after
/// the view's own, the wildcards match, a row finds the sequences the
/// stack has and only those, and a swapped walk keeps the sim's moment and
/// pace over the other frames.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let sequence = |name: &str, first_frame, frame_count| Sequence {
        name: name.to_string(),
        first_frame,
        frame_count,
        loop_frame: 0,
        frames_per_second: 20,
    };
    let stack: BTreeMap<String, Sequence> = [
        sequence("BOTH_SABERSTAFF_STANCE", 100, 1),
        sequence("BOTH_SABERSTAFF_STANCE_TEST", 500, 40),
        sequence("BOTH_WALK2", 200, 30),
    ]
    .into_iter()
    .map(|s| (s.name.clone(), s))
    .collect();
    let test = Row {
        name: "test".into(),
        tags: vec!["test".into()],
        suffixes: vec!["_MISSING".into(), "_TEST".into()],
        ..Row::default()
    };
    let mut stances = DuelStances {
        swaps: true,
        ..DuelStances::default()
    };
    stances.of(&[test], |_, name| stack.get(name).cloned(), &mut Vec::new());
    let swaps: Vec<(&str, &str)> = stances
        .looks
        .iter()
        .flat_map(|look| &look.swaps)
        .map(|(anim, sequence)| (anim.name(), sequence.name.as_str()))
        .collect();
    let walk = &stack["BOTH_WALK2"];
    let shown = Shown {
        anim: Anim::BOTH_WALK2,
        flip: false,
        playback: Playback::set(None, &crate::stage::sim_sequence(walk), 1.5, 1000, 0, None),
        plays: sim::skeleton::Played::OWN,
    };
    let replay = replayed(shown, Some(walk), &sequence("BOTH_WALK2_TEST", 700, 20)).playback;
    let kept = (replay.start_frame, replay.end_frame, replay.start_ms) == (700, 720, 1000)
        && (replay.speed - shown.playback.speed).abs() < 1e-4;
    // Two frames of a two-bone skeleton lent to a one-frame one: added
    // after its own, each bone's transform the lender's.
    let skeleton = |frames: &[[u16; 7]]| Gla {
        name: "probe".into(),
        scale: 1.0,
        bones: (0..2)
            .map(|n| fmt_ja::ghoul2::Bone {
                name: format!("bone{n}"),
                flags: 0,
                parent: n - 1,
                base_pose: [[0.0; 4]; 3],
                base_pose_inverse: [[0.0; 4]; 3],
                children: Vec::new(),
            })
            .collect(),
        frame_count: frames.len() / 2,
        frames: (0..frames.len() as u32).collect(),
        pool: frames.to_vec(),
    };
    let mut into = skeleton(&[[1; 7], [2; 7]]);
    let from = skeleton(&[[3; 7], [4; 7], [5; 7], [6; 7], [7; 7], [8; 7]]);
    let lent = append(&mut into, &from, &sequence("LENT", 1, 2));
    let lent_frames: Vec<u16> = (0..into.frame_count * 2)
        .map(|i| into.pool[into.frames[i] as usize][0])
        .collect();
    let lends = lent
        .as_ref()
        .is_some_and(|s| s.first_frame == 1 && s.frame_count == 2)
        && lent_frames == [1, 2, 5, 6, 7, 8];
    // A held block's poses, as the stack has them: a staff's guard, no
    // single blade's aimed parry (the stance plays on).
    let by_name = |sequences: Vec<Sequence>| -> BTreeMap<String, Sequence> {
        sequences.into_iter().map(|s| (s.name.clone(), s)).collect()
    };
    let plain = DuelStances::plain(&by_name(vec![sequence(
        "BOTH_STAND_BLOCKING_ON_STAFF",
        900,
        2,
    )]));
    let guarded = (
        plain
            .guard(None, Anim::BOTH_SABERSTAFF_STANCE, false, (0, 0))
            .map(|s| s.name.as_str()),
        plain.guard(None, Anim::BOTH_STAND2, true, (0, 0)).is_none(),
    );
    // Moving with block held: the parry toward the move, every way;
    // standing without a guard of its own, the top parry; a pair's own.
    let ways = [
        ((-1, -1), "TL"),
        ((-1, 1), "TR"),
        ((-1, 0), "T_"),
        ((1, -1), "BL"),
        ((1, 1), "BR"),
        ((1, 0), "T_"),
        ((0, -1), "TL"),
        ((0, 1), "TR"),
    ];
    let parries = DuelStances::plain(&by_name(
        PARRIES
            .iter()
            .flat_map(|parry| {
                QUADRANTS.map(|quadrant| sequence(&format!("{parry}{quadrant}"), 1000, 4))
            })
            .collect(),
    ));
    let faced = |stances: &DuelStances, anim: Anim, aimed: bool| -> Vec<String> {
        ways.iter()
            .map(|(way, _)| {
                stances
                    .guard(None, anim, aimed, *way)
                    .map_or(String::new(), |s| s.name.clone())
            })
            .collect()
    };
    let wanted: Vec<String> = ways
        .iter()
        .map(|(_, way)| format!("BOTH_P1_S1_{way}"))
        .collect();
    let still = (
        Quadrant::of(0, 0),
        parries
            .guard(None, Anim::BOTH_STAND2, false, (0, 0))
            .map(|s| s.name.as_str()),
        parries
            .guard(None, Anim::BOTH_WALK_DUAL, false, (1, 1))
            .map(|s| s.name.as_str()),
    );
    let faces = faced(&parries, Anim::BOTH_WALKBACK2, false) == wanted
        && still == (None, Some("BOTH_P1_S1_T_"), Some("BOTH_P6_S6_BR"));
    // With Movie Duels' guards for each step as well: moving with block
    // alone, the guard for the step (a side over forward or back); with
    // attack too, the parry toward the move as before; still, the guard.
    // Each guard plays frames of its own (`Guards::own_weapons`).
    let guard_names = || {
        STANDING.iter().flat_map(|standing| {
            std::iter::once(standing.to_string())
                .chain(STEPS.map(|step| format!("{standing}{step}")))
        })
    };
    let own_frames = |names: Vec<String>| -> Vec<Sequence> {
        (1100..)
            .step_by(2)
            .zip(names)
            .map(|(first, name)| sequence(&name, first, 2))
            .collect()
    };
    let stepped = {
        let mut sequences = own_frames(guard_names().collect());
        sequences.extend(parries.guards.parries.iter().flatten().flatten().cloned());
        DuelStances::plain(&by_name(sequences))
    };
    let steps = [
        "_LEFT", "_RIGHT", "_BACK", "_LEFT", "_RIGHT", "_FORWARD", "_LEFT", "_RIGHT",
    ];
    let stepped_wanted: Vec<String> = steps
        .iter()
        .map(|step| format!("BOTH_STAND_BLOCKING_ON_DUAL{step}"))
        .collect();
    let steps_shown = faced(&stepped, Anim::BOTH_WALK_DUAL, false) == stepped_wanted
        && faced(&stepped, Anim::BOTH_WALKBACK2, true) == wanted
        && stepped
            .guard(None, Anim::BOTH_SABERDUAL_STANCE, false, (0, 0))
            .is_some_and(|s| s.name == "BOTH_STAND_BLOCKING_ON_DUAL");
    // An aimed staff block standing still shows the staff's standing guard;
    // moving, the parry toward the move.
    let staff_aimed = [
        (Anim::BOTH_SABERSTAFF_STANCE, (0, 0)),
        (Anim::BOTH_WALK_STAFF, (1, 0)),
        (Anim::BOTH_SABERDUAL_STANCE, (0, 0)),
    ]
    .map(|(anim, way)| stepped.guard(None, anim, true, way).map(|s| s.name.clone()));
    // A guard for two blades or the staff on the one-blade guard's frames
    // counts as missing: a copied standing guard takes all of its weapon's
    // guards with it, a copied moving guard only itself.
    let copies = {
        let mut sequences = own_frames(guard_names().collect());
        let frames_of = |sequences: &[Sequence], name: &str| {
            let found = sequences.iter().find(|s| s.name == name).unwrap();
            (found.first_frame, found.frame_count)
        };
        let copied = [
            ("BOTH_STAND_BLOCKING_ON_STAFF", "BOTH_STAND_BLOCKING_ON"),
            (
                "BOTH_STAND_BLOCKING_ON_DUAL_LEFT",
                "BOTH_STAND_BLOCKING_ON_LEFT",
            ),
        ];
        for (copy, of) in copied {
            let (first, count) = frames_of(&sequences, of);
            let at = sequences.iter_mut().find(|s| s.name == copy).unwrap();
            (at.first_frame, at.frame_count) = (first, count);
        }
        let guards = guards(|name| sequences.iter().find(|s| s.name == name).cloned());
        let kept = |guard: &Option<Sequence>| guard.is_some();
        (
            guards.standing.each_ref().map(kept),
            guards
                .moving
                .each_ref()
                .map(|moving| moving.each_ref().map(kept)),
        )
    };
    let copies_wanted = (
        [true, true, false],
        [[true; 4], [true, true, false, true], [false; 4]],
    );
    // A perfect parry: Movie Duels' knockaway to the side the blow was met
    // on. A blow met at the top, two blades and a stack without Movie
    // Duels' knockaways keep JA's.
    let perfect = {
        let knockaways = PERFECT.map(|(_, name)| sequence(name, 1100, 13));
        let with = DuelStances::plain(&by_name(knockaways.to_vec()));
        let over = [
            Anim::BOTH_K1_S1_TL,
            Anim::BOTH_K1_S1_TR,
            Anim::BOTH_K1_S1_T_,
            Anim::BOTH_K6_S6_TL,
        ];
        let shown = |stances: &DuelStances| -> Vec<Option<String>> {
            over.iter()
                .map(|&anim| stances.perfect(None, anim).map(|s| s.name.clone()))
                .collect()
        };
        let found = (shown(&with), shown(&plain));
        let wanted = vec![
            Some("BOTH_K1_S1_TL_MD".to_string()),
            Some("BOTH_K1_S1_TR_MD".to_string()),
            None,
            None,
        ];
        check(
            found == (wanted, vec![None; over.len()]),
            format!("{found:?}"),
        )
    };
    // A pick: the player given a skeleton of its own over its row, read
    // once; picked again, nothing changes; another, the first stays read;
    // with the stances off, no pick counts.
    let picked = {
        let row = Row {
            name: "the player".into(),
            player: true,
            skeleton: Some("_humanoid_kotor".into()),
            ..Row::default()
        };
        let lends = |folder: &str, name: &str| {
            (name == "BOTH_STAND2").then(|| sequence(&format!("{folder} STAND2"), 1200, 10))
        };
        let mut stances = DuelStances {
            swaps: true,
            ..DuelStances::default()
        };
        stances.of(
            &[row],
            |row, name| lends(row.skeleton.as_deref()?, name),
            &mut Vec::new(),
        );
        let mut reads = 0;
        let mut pick = |stances: &mut DuelStances, folder: &str| {
            let picks = [("seat0".to_string(), folder.to_string())].into();
            stances.pick(
                picks,
                |folder, name| {
                    reads += usize::from(name == "BOTH_STAND2");
                    lends(folder, name)
                },
                &mut Vec::new(),
            )
        };
        let shown = |stances: &DuelStances| {
            let look = stances.look_for(Some("seat0"), None, true);
            stances
                .swap(look, Anim::BOTH_STAND2)
                .map_or(String::new(), |s| s.name.clone())
        };
        let before = shown(&stances);
        let first = (pick(&mut stances, "_humanoid_obi"), shown(&stances));
        let again = pick(&mut stances, "_humanoid_obi");
        let other = (pick(&mut stances, "_humanoid_maul"), shown(&stances));
        let back = (pick(&mut stances, "_humanoid_obi"), shown(&stances));
        stances.swaps = false;
        let off = pick(&mut stances, "_humanoid_maul");
        let unpicked = stances.look_for(Some("seat0"), None, true) == Some(0);
        let found = format!(
            "{before:?}, picked {first:?}, again {again}, another {other:?}, back {back:?}, \
             skeletons read {reads}, off {off} {unpicked}"
        );
        let right = before == "_humanoid_kotor STAND2"
            && first == (true, "_humanoid_obi STAND2".into())
            && !again
            && other == (true, "_humanoid_maul STAND2".into())
            && back == (true, "_humanoid_obi STAND2".into())
            && reads == 2
            && off
            && unpicked;
        check(right, found)
    };
    vec![
        (
            "a held block shows the stack's guard for its blades",
            check(
                guarded == (Some("BOTH_STAND_BLOCKING_ON_STAFF"), true),
                format!("{guarded:?}"),
            ),
        ),
        (
            "a held block parries toward its move, standing its guard",
            check(
                faces,
                format!(
                    "{:?}, still {still:?}",
                    faced(&parries, Anim::BOTH_WALKBACK2, false)
                ),
            ),
        ),
        (
            "a held block without attack shows the guard for its step",
            check(
                steps_shown,
                format!("{:?}", faced(&stepped, Anim::BOTH_WALK_DUAL, false)),
            ),
        ),
        (
            "an aimed staff block standing still shows the staff's guard",
            check(
                staff_aimed
                    == [
                        Some("BOTH_STAND_BLOCKING_ON_STAFF".into()),
                        Some("BOTH_P7_S7_T_".into()),
                        Some("BOTH_P6_S6_T_".into()),
                    ],
                format!("{staff_aimed:?}"),
            ),
        ),
        (
            "a guard copied from the one-blade guard counts as missing",
            check(copies == copies_wanted, format!("{copies:?}")),
        ),
        (
            "a perfect parry shows Movie Duels' knockaway to its side",
            perfect,
        ),
        (
            "a changed pick stands the character in its skeleton",
            picked,
        ),
        (
            "a skeleton's frames are lent after the view's own",
            check(lends, format!("{lent:?}, frames {lent_frames:?}")),
        ),
        (
            "a row draws only the sequences the stack has",
            check(
                swaps == [("BOTH_SABERSTAFF_STANCE", "BOTH_SABERSTAFF_STANCE_TEST")],
                format!("{swaps:?}"),
            ),
        ),
        (
            "a swapped walk keeps the sim's moment and pace",
            check(kept, format!("{replay:?} from {:?}", shown.playback)),
        ),
    ]
}
