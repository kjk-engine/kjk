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
//! - **Without it** (the option off, no such sequence in the stack, no
//!   blade lit, a body drawn the cgame's way): JA's sequence plays.
//!
//! Render only, as `stance`: the sim's skeleton, which the blades cut
//! with, holds JA's sequence, so no digest moves.

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_ja::animation_cfg::{AnimationConfig, Sequence};
use fmt_ja::ghoul2::Gla;
use sim::combat::saber::blades::Shown;
use sim::motion::Anim;
use sim::skeleton::Playback;
use sim::{EntityState, Instances, Kind};

/// The setting that turns them on (`true`).
pub const SETTING: &str = "game.duel_stances";

/// The settings a character's own pick is kept in: this and its `who`
/// (`headgear::who`: a seat's character by its seat, any other by its
/// tag), its value a skeleton's folder (`_humanoid_obi`); unset or empty,
/// its row's. In the player's settings, as the headgear's choice: a look
/// of their own game alone, never the sim's, kept across saves.
pub const PICKED: &str = "saber.stance.";

/// The setting `who`'s pick is kept in.
pub fn pick_setting(who: &str) -> String {
    format!("{PICKED}{who}")
}

/// The picks among `values` (a settings map's), by `who`.
pub fn picks<'a>(
    values: impl IntoIterator<Item = (&'a String, &'a String)>,
) -> BTreeMap<String, String> {
    values
        .into_iter()
        .filter_map(|(key, folder)| Some((key.strip_prefix(PICKED)?, folder)))
        .filter(|(_, folder)| !folder.is_empty())
        .map(|(who, folder)| (who.to_string(), folder.to_ascii_lowercase()))
        .collect()
}

/// Where Movie Duels keeps its characters' skeletons: a folder each, its
/// `_humanoid.gla` beside its `animation.cfg` (Jedi Academy's own
/// cutscene skeletons are named after their folders).
const SKELETONS: &str = "models/players/_humanoid_";

/// Its skeletons not offered: its droids' and creatures'.
const NOT_OFFERED: [&str; 4] = [
    "_humanoid_bdroid",
    "_humanoid_sbd",
    "_humanoid_deka",
    "_humanoid_jabba",
];

/// The names the character page shows for the skeletons it knows; any
/// other by its folder's last word.
const TITLES: [(&str, &str); 20] = [
    ("kotor", "KOTOR Jedi"),
    ("md", "Movie Duels"),
    ("ani", "Anakin"),
    ("ben", "Ben Kenobi"),
    ("obi", "Obi-Wan"),
    ("obi3", "Obi-Wan III"),
    ("yoda", "Yoda"),
    ("vader", "Vader"),
    ("maul", "Maul"),
    ("mace", "Mace Windu"),
    ("cal", "Cal Kestis"),
    ("rey", "Rey"),
    ("dooku", "Dooku"),
    ("galen", "Galen Marek"),
    ("ren", "Kylo Ren"),
    ("luke", "Luke"),
    ("gon", "Qui-Gon"),
    ("grievous", "Grievous"),
    ("jango", "Jango Fett"),
    ("pal", "Palpatine"),
];

/// The name shown for the skeleton in `folder`.
pub fn title(folder: &str) -> String {
    let short = folder.strip_prefix("_humanoid_").unwrap_or(folder);
    match TITLES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(short))
    {
        Some((_, title)) => title.to_string(),
        None => {
            let mut letters = short.chars();
            letters.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + letters.as_str()
            })
        }
    }
}

/// The skeletons `files` offer for picking, by folder: Movie Duels'
/// (`SKELETONS`), but its droids' and creatures'.
fn offered(files: &crate::viewer_ja::Files) -> Vec<String> {
    let mut folders: Vec<String> = files
        .names()
        .filter_map(|path| {
            let folder = path
                .strip_prefix("models/players/")?
                .strip_suffix("/_humanoid.gla")?;
            Some(folder.to_ascii_lowercase())
        })
        .filter(|folder| folder.starts_with(&SKELETONS["models/players/".len()..]))
        .filter(|folder| !NOT_OFFERED.contains(&folder.as_str()))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

/// The skeletons offered for picking, by folder, in the Jedi Academy
/// install that `discovery` finds (`offered`).
pub fn offered_folders(discovery: &vfs::Discovery) -> Result<Vec<String>, String> {
    let stack = crate::viewer_ja::stack(discovery)?;
    Ok(offered(&crate::viewer_ja::Files::new(&stack)))
}

/// The table, as shipped.
const TABLE: &str = include_str!("../../../data/duel_stances.toml");

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

/// The sim's walks a lit blade's legs play, forward and back: under a held
/// block the legs walk on.
const WALKS: [Anim; 6] = [
    Anim::BOTH_WALK2,
    Anim::BOTH_WALK_DUAL,
    Anim::BOTH_WALK_STAFF,
    Anim::BOTH_WALKBACK2,
    Anim::BOTH_WALKBACK_DUAL,
    Anim::BOTH_WALKBACK_STAFF,
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

/// A held block's poses (`STANDING`, `PARRIES`), by blades.
#[derive(Debug, Clone, Default, PartialEq)]
struct Guards {
    standing: [Option<Sequence>; 3],
    parries: [[Option<Sequence>; 5]; 3],
}

impl Guards {
    fn any(&self) -> bool {
        self.standing
            .iter()
            .chain(self.parries.iter().flatten())
            .any(Option::is_some)
    }
}

/// `STANDING`'s and `PARRIES`' sequences as `find` finds them.
fn guards(mut find: impl FnMut(&str) -> Option<Sequence>) -> Guards {
    Guards {
        standing: STANDING.map(&mut find),
        parries: PARRIES.map(|parry| QUADRANTS.map(|quadrant| find(&format!("{parry}{quadrant}")))),
    }
}

/// One row of the table, read.
#[derive(Debug, Clone, Default, PartialEq)]
struct Row {
    name: String,
    templates: Vec<String>,
    tags: Vec<String>,
    player: bool,
    suffixes: Vec<String>,
    /// The skeleton its sequences are looked up in (`_humanoid_kotor`, a
    /// folder of `models/players/`), else the stack's own.
    skeleton: Option<String>,
    /// JA's name (upper case) to the one drawn, over the suffixes.
    sequences: BTreeMap<String, String>,
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
    /// first, else the stack's), by the blades `anim` holds: moving toward
    /// `quadrant`, its parry; standing, with attack held (`aimed`) the top
    /// parry, else the standing guard, the top parry where there is none.
    pub fn guard(
        &self,
        look: Option<u16>,
        anim: Anim,
        aimed: bool,
        quadrant: Option<Quadrant>,
    ) -> Option<&Sequence> {
        let blades = match anim {
            Anim::BOTH_SABERDUAL_STANCE | Anim::BOTH_WALK_DUAL | Anim::BOTH_WALKBACK_DUAL => 1,
            Anim::BOTH_SABERSTAFF_STANCE | Anim::BOTH_WALK_STAFF | Anim::BOTH_WALKBACK_STAFF => 2,
            _ => 0,
        };
        let own = look
            .and_then(|look| self.looks.get(usize::from(look)))
            .map(|look| &look.guards);
        let parry = quadrant.or(aimed.then_some(Quadrant::Top));
        self.pose(own, blades, parry)
            .or_else(|| self.pose(own, blades, Some(Quadrant::Top)))
    }

    /// A held block's pose for `blades`, `own` (a look's) first, else the
    /// stack's: the parry toward `parry`, none the standing guard.
    fn pose<'s>(
        &'s self,
        own: Option<&'s Guards>,
        blades: usize,
        parry: Option<Quadrant>,
    ) -> Option<&'s Sequence> {
        let of = |guards: &'s Guards| match parry {
            Some(quadrant) => guards.parries[blades][quadrant as usize].as_ref(),
            None => guards.standing[blades].as_ref(),
        };
        own.and_then(of).or_else(|| of(&self.guards))
    }

    /// Whether `anim` is a walk, which the legs keep under a held block.
    pub fn walks(anim: Anim) -> bool {
        WALKS.contains(&anim)
    }

    /// Whether a held block's pose shows over `anim`: a lit blade's ready
    /// stance, fidget or walk, or its lowering to rest and back (the sim's
    /// idle blade lowers while block is held, swinging nothing).
    pub fn guarded(anim: Anim) -> bool {
        SWAPPED.contains(&anim)
            || WALKS.contains(&anim)
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
    notes: Vec<String>,
}

impl<'g> Borrowed<'g> {
    fn new(gla: &'g Gla) -> Borrowed<'g> {
        Borrowed {
            gla,
            extended: None,
            read: BTreeMap::new(),
            taken: BTreeMap::new(),
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
                lender(files, gla, &key.0)
                    .inspect_err(|error| notes.push(format!("duel stances, {}: {error}", key.0)))
                    .ok()
            })
            .as_ref();
        let taken = lent.and_then(|(from, sequences)| {
            let sequence = sequences.get(name)?;
            let into = self.extended.get_or_insert_with(|| gla.clone());
            append(into, from, sequence)
        });
        self.taken.insert(key, taken.clone());
        taken
    }
}

/// A skeleton that lends its frames, and its sequences by upper-case name.
type Lender = (Gla, BTreeMap<String, Sequence>);

/// The skeleton in `models/players/<folder>/` (its `_humanoid.gla`, or
/// the one named as its folder) and its `animation.cfg`, when its bones
/// are `gla`'s, in order.
fn lender(files: &crate::viewer_ja::Files, gla: &Gla, folder: &str) -> Result<Lender, String> {
    let base = format!("models/players/{folder}");
    let (path, bytes) = ["_humanoid", folder]
        .iter()
        .map(|stem| format!("{base}/{stem}.gla"))
        .find_map(|path| files.read(&path).map(|bytes| (path, bytes)))
        .ok_or_else(|| format!("{base}: no skeleton"))?;
    let lent = Gla::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let same = lent.bones.len() == gla.bones.len()
        && lent
            .bones
            .iter()
            .zip(&gla.bones)
            .all(|(a, b)| a.name.eq_ignore_ascii_case(&b.name));
    if !same {
        return Err(format!(
            "{path}: its {} bones are not the skeleton's {}",
            lent.bones.len(),
            gla.bones.len()
        ));
    }
    let cfg = files
        .read(&format!("{base}/animation.cfg"))
        .map(|bytes| AnimationConfig::parse(&bytes))
        .ok_or_else(|| format!("{base}/animation.cfg: not found"))?;
    let sequences = cfg
        .sequences
        .into_iter()
        .map(|s| (s.name.to_ascii_uppercase(), s))
        .collect();
    Ok((lent, sequences))
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

/// Whether `name` matches `pattern`, any case, `*` standing for any run
/// of characters.
fn glob(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The table's rows: `[default]` first, then each `[[character]]`.
fn rows(text: &str) -> Result<Vec<Row>, String> {
    use toml_edit::{Document, Item};
    let doc = Document::parse(text).map_err(|error| error.to_string())?;
    let strings = |item: Option<&Item>| -> Vec<String> {
        item.and_then(Item::as_array)
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()
    };
    let row = |table: &dyn toml_edit::TableLike, name: &str| Row {
        name: table
            .get("name")
            .and_then(Item::as_str)
            .unwrap_or(name)
            .to_string(),
        templates: strings(table.get("templates")),
        tags: strings(table.get("tags")),
        player: table.get("player").and_then(Item::as_bool).unwrap_or(false),
        suffixes: strings(table.get("suffixes")),
        skeleton: table
            .get("skeleton")
            .and_then(Item::as_str)
            .map(str::to_string),
        sequences: table
            .get("sequences")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(|named| named.iter())
            .filter_map(|(ja, drawn)| Some((ja.to_ascii_uppercase(), drawn.as_str()?.to_string())))
            .collect(),
    };
    let mut rows = Vec::new();
    if let Some(default) = doc.get("default").and_then(Item::as_table_like) {
        rows.push(Row {
            templates: Vec::new(),
            tags: Vec::new(),
            player: false,
            ..row(default, "everyone else")
        });
    }
    let characters = doc
        .get("character")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten();
    for (n, character) in characters.enumerate() {
        let read = row(character, &format!("row {}", n + 1));
        if read.templates.is_empty() && read.tags.is_empty() && !read.player {
            return Err(format!("{}: names no template, tag or player", read.name));
        }
        rows.push(read);
    }
    Ok(rows)
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
    let table = match rows(TABLE) {
        Ok(rows) => Ok(format!("{} rows", rows.len())),
        Err(error) => Err(error),
    };
    let globs = [
        ("p_bastilla*", "P_Bastilla001", true),
        ("p_bastilla*", "p_bastil", false),
        ("*malak*", "end_darthmalak", true),
        ("bastila", "Bastila", true),
        ("bastila", "bastila2", false),
    ];
    let wrong: Vec<_> = globs
        .iter()
        .filter(|(pattern, name, want)| glob(pattern, name) != *want)
        .collect();
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
            .guard(None, Anim::BOTH_SABERSTAFF_STANCE, false, None)
            .map(|s| s.name.as_str()),
        plain.guard(None, Anim::BOTH_STAND2, true, None).is_none(),
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
    let faced: Vec<String> = ways
        .iter()
        .map(|((forward, right), _)| {
            let quadrant = Quadrant::of(*forward, *right);
            parries
                .guard(None, Anim::BOTH_WALKBACK2, false, quadrant)
                .map_or(String::new(), |s| s.name.clone())
        })
        .collect();
    let wanted: Vec<String> = ways
        .iter()
        .map(|(_, way)| format!("BOTH_P1_S1_{way}"))
        .collect();
    let still = (
        Quadrant::of(0, 0),
        parries
            .guard(None, Anim::BOTH_STAND2, false, None)
            .map(|s| s.name.as_str()),
        parries
            .guard(None, Anim::BOTH_WALK_DUAL, false, Quadrant::of(1, 1))
            .map(|s| s.name.as_str()),
    );
    let faces = faced == wanted && still == (None, Some("BOTH_P1_S1_T_"), Some("BOTH_P6_S6_BR"));
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
        ("the shipped table reads", table),
        (
            "a held block shows the stack's guard for its blades",
            check(
                guarded == (Some("BOTH_STAND_BLOCKING_ON_STAFF"), true),
                format!("{guarded:?}"),
            ),
        ),
        (
            "a held block parries toward its move, standing its guard",
            check(faces, format!("{faced:?}, still {still:?}")),
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
            "wildcards match as written",
            check(
                wrong.is_empty(),
                format!("{} cases, wrong {wrong:?}", globs.len()),
            ),
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
