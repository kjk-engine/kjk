//! `kjk anim audit`: the audit's cases, each one `kjk play` run of the
//! arena's duelist through one script of saber states, and the run of them
//! all. Each case's frames are cut into its script's states by their moment
//! and measured state by state (`checks::measure`). The cases and their
//! worst values go to `cases.json` and `report.md` (`report`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::checks::measure;
use super::io;
use super::report::{self, Record, StateRecord};

/// The saber or sabers the duelist carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weapon {
    Single,
    Staff,
    Dual,
}

/// The KOTOR body the duelist wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Body {
    Male,
    Female,
}

/// The saber rules a case plays by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rules {
    /// Jedi Academy's rules.
    Ja,
    /// Movie Duels Advanced.
    Mda,
}

impl Weapon {
    pub const ALL: [Weapon; 3] = [Weapon::Single, Weapon::Staff, Weapon::Dual];

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Single => "single",
            Weapon::Staff => "staff",
            Weapon::Dual => "dual",
        }
    }

    /// The `--class` that gives the duelist this weapon.
    fn class(self) -> &'static str {
        match self {
            Weapon::Single => "duelist",
            Weapon::Staff => "duelist@staff",
            Weapon::Dual => "duelist@dual",
        }
    }
}

impl Body {
    pub const ALL: [Body; 2] = [Body::Male, Body::Female];

    pub fn name(self) -> &'static str {
        match self {
            Body::Male => "male",
            Body::Female => "female",
        }
    }

    /// The KOTOR body model `--body` takes.
    fn model(self) -> &'static str {
        match self {
            Body::Male => "pmbal",
            Body::Female => "pfbam",
        }
    }

    /// The KOTOR head model `--head` takes, a player's head of the body's
    /// sex.
    fn head(self) -> &'static str {
        match self {
            Body::Male => "pmha01",
            Body::Female => "pfha01",
        }
    }
}

impl Rules {
    /// The value `--saber-rules` takes.
    pub fn name(self) -> &'static str {
        match self {
            Rules::Ja => "ja",
            Rules::Mda => "mda",
        }
    }
}

/// The stance of a case that plays with the duel stances off, so the drawn
/// body plays JA's own sequences.
pub const JA_STANCE: &str = "ja";

/// One audit case: the duelist in one stance, with one weapon and body,
/// under one set of saber rules, playing `SCRIPT`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    /// A Movie Duels skeleton's folder (`_humanoid_obi`), or `JA_STANCE`.
    pub stance: String,
    pub weapon: Weapon,
    pub body: Body,
    pub rules: Rules,
}

/// The states of the script every case plays, in order, as (label,
/// `--moves` segment, still). A still state is one in which the hands
/// should not move (`Check::Stillness`). The blade is holstered at the
/// start of `holster` (`HOLSTER`, through `--cmds "ui holster"`), and the
/// attack held in `ignite` lights it again. `LEAD_IN` is played before a
/// state where the blade would be put away (`timeline`).
pub const SCRIPT: &[(&str, &str, bool)] = &[
    ("stand", "2s", true),
    ("walk", "2s w walk", false),
    ("run", "2s w", false),
    ("block_stand", "2s block", true),
    ("block_stand_aim", "2s block attack", true),
    ("block_walk_f", "2s w walk block", false),
    ("block_walk_b", "2s s walk block", false),
    ("block_walk_l", "2s a walk block", false),
    ("block_walk_r", "2s d walk block", false),
    ("attack_f", "1.2s attack w", false),
    ("attack_b", "1.2s attack s", false),
    ("attack_l", "1.2s attack a", false),
    ("attack_r", "1.2s attack d", false),
    ("attack", "1.2s attack", false),
    (HOLSTER, "2s", false),
    ("ignite", "1.5s attack", false),
];

/// Whether the duel stances swap the drawn body's sequences in the state
/// `label` of `SCRIPT`. They swap only the ready stances, the walks and the
/// held-block guard (`kjk_game::duel_stances`). The attacks, the run, the
/// holster and the ignition play JA's sequences by design.
fn swapped_by_stances(label: &str) -> bool {
    matches!(label, "stand" | "walk") || label.starts_with("block_")
}

/// The state at whose start the blade is holstered. It is the one state
/// that may have no lit blade (`StateRecord::no_lit_blade`).
pub const HOLSTER: &str = "holster";

/// The `--moves` segments played before a state that needs them, not
/// measured. The duelist comes in with the blade put away, and KOTOR draws
/// a weapon only in a fight. An attack lights the blade and starts a fight,
/// and the swing is back in the stance before the next state starts.
const LEAD_IN: &str = "1.2s attack; 0.8s";

/// A fight lasts `combat.timing.combat_ms` (8 s) after the last attack,
/// and then the blade is put away. A state that would end later than this
/// after the last attack gets `LEAD_IN` before it, 2 s inside the fight's
/// end.
const LIT_FOR_MS: u64 = 6000;

/// The step of game time each run is held at, one after another (ms). A
/// run's drawn moments then follow the game's time and not the machine's
/// pace, so two runs of a case draw the same frames.
pub const STEP_MS: u64 = 25;

/// How long a run may take before it is stopped and counted as failed.
const RUN_LIMIT: Duration = Duration::from_secs(400);

/// How many runs play at the same time.
const AT_ONCE: usize = 4;

/// How long `segments` last (ms).
fn length_ms(segments: &str) -> u64 {
    sim::moves::Script::parse(segments)
        .expect("the audit's own segments parse")
        .length_ms()
}

/// Whether `segment` holds the attack button, which starts or renews a
/// fight.
fn attacks(segment: &str) -> bool {
    segment.split_whitespace().any(|word| word == "attack")
}

/// One state of a script and the moments of the run it spans (ms, from its
/// own start to its end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub label: &'static str,
    pub from: u64,
    pub to: u64,
    pub still: bool,
}

/// A run of `script`: its `--moves` segments, `LEAD_IN` among them where a
/// state needs it, and the spans of its states, which skip the lead-ins. A
/// state that does not attack gets `LEAD_IN` before it when no attack came
/// before it, or when it would end more than `LIT_FOR_MS` after the last
/// attack. An attack is counted from its segment's start.
pub fn timeline(script: &[(&'static str, &'static str, bool)]) -> (Vec<&'static str>, Vec<Span>) {
    let lead_in = length_ms(LEAD_IN);
    let mut moves = Vec::new();
    let mut spans = Vec::new();
    let mut at = 0;
    let mut last_attack: Option<u64> = None;
    for &(label, segment, still) in script {
        let length = length_ms(segment);
        let lit_through = last_attack.is_some_and(|attack| at + length <= attack + LIT_FOR_MS);
        if !attacks(segment) && !lit_through {
            moves.push(LEAD_IN);
            last_attack = Some(at);
            at += lead_in;
        }
        if attacks(segment) {
            last_attack = Some(at);
        }
        moves.push(segment);
        spans.push(Span {
            label,
            from: at,
            to: at + length,
            still,
        });
        at += length;
    }
    (moves, spans)
}

/// The moment of `SCRIPT`'s run at which the blade is holstered (ms), the
/// start of `HOLSTER`.
fn holster_ms() -> Option<u64> {
    timeline(SCRIPT)
        .1
        .into_iter()
        .find(|span| span.label == HOLSTER)
        .map(|span| span.from)
}

/// The `--cmds` that step a run of `SCRIPT` to `until` (ms). The game's
/// time is held at 0 from the start, then at each `STEP_MS` step in turn up
/// to `until`, and each step is waited for. The blade is holstered once the
/// world has reached `holster_ms`. After each step, and after the holster
/// at its step, come the commands `after` gives for the step's moment (ms).
pub fn steps(until: u64, mut after: impl FnMut(u64) -> Vec<String>) -> Vec<String> {
    let holster = holster_ms();
    let mut cmds = vec!["anim 0".to_string()];
    cmds.extend(after(0));
    for ms in (STEP_MS..=until).step_by(STEP_MS as usize) {
        cmds.push(format!("anim {}", ms as f64 / 1000.0));
        cmds.push("wait arrived".to_string());
        if holster == Some(ms) {
            cmds.push("ui holster".to_string());
        }
        cmds.extend(after(ms));
    }
    cmds
}

/// `ms` rounded down to a whole `STEP_MS` step, which `steps` can reach.
pub fn step_floor(ms: u64) -> u64 {
    ms / STEP_MS * STEP_MS
}

/// The label of the state of `script` that runs at `ms` of its run, or none
/// in a lead-in and after the script's end.
pub fn label(script: &[(&'static str, &'static str, bool)], ms: f64) -> Option<&'static str> {
    timeline(script)
        .1
        .into_iter()
        .find(|span| (span.from as f64..span.to as f64).contains(&ms))
        .map(|span| span.label)
}

/// Every case: each skeleton in `stances` (Movie Duels' folders) and
/// `JA_STANCE`, with each weapon and body, under MDA; and the six cases of
/// `JA_STANCE` under JA's rules.
pub fn cases(stances: &[String]) -> Vec<Case> {
    let mda = stances
        .iter()
        .map(String::as_str)
        .chain([JA_STANCE])
        .map(|stance| (stance, Rules::Mda));
    mda.chain([(JA_STANCE, Rules::Ja)])
        .flat_map(|(stance, rules)| {
            Weapon::ALL.into_iter().flat_map(move |weapon| {
                Body::ALL.into_iter().map(move |body| Case {
                    stance: stance.to_string(),
                    weapon,
                    body,
                    rules,
                })
            })
        })
        .collect()
}

impl Case {
    /// The stance's short name: the skeleton's folder without
    /// `_humanoid_` (`obi`), or `JA_STANCE`.
    pub fn stance_name(&self) -> &str {
        self.stance
            .strip_prefix("_humanoid_")
            .unwrap_or(&self.stance)
    }

    /// The words that name the case, which `--only` matches: the stance's
    /// short name, the weapon, the body, and `ja-rules` or `mda-rules`.
    fn tags(&self) -> [String; 4] {
        [
            self.stance_name().to_string(),
            self.weapon.name().to_string(),
            self.body.name().to_string(),
            format!("{}-rules", self.rules.name()),
        ]
    }

    /// The case's name, its tags one after another (`obi single male
    /// mda-rules`).
    pub fn name(&self) -> String {
        self.tags().join(" ")
    }

    /// Whether `filter` chooses the case. The filter is a comma list of
    /// words, and each word must be one of the case's tags.
    fn chosen(&self, filter: &str) -> bool {
        let tags = self.tags();
        filter
            .split(',')
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .all(|word| tags.iter().any(|tag| tag == word))
    }

    /// Whether the duel stances are on, so the drawn body plays the
    /// skeleton's sequences where it has them.
    fn stances_on(&self) -> bool {
        self.stance != JA_STANCE
    }

    /// The settings file's text: the duel stances on with the skeleton
    /// picked for the first seat, or off.
    fn settings(&self) -> serde_json::Result<String> {
        let mut settings = BTreeMap::new();
        settings.insert("game.duel_stances", self.stances_on().to_string());
        if self.stances_on() {
            settings.insert("saber.stance.seat0", self.stance.clone());
        }
        serde_json::to_string_pretty(&settings)
    }

    /// The `kjk` arguments of the case's run, writing its frames into
    /// `dir`.
    fn args(&self, dir: &Path) -> Vec<String> {
        let (moves, spans) = timeline(SCRIPT);
        let end = spans.last().map_or(0, |span| span.to);
        // The steps go on to half a second past the script's end, so its
        // last state is drawn to its end.
        let mut cmds = steps(end + 500, |_| Vec::new());
        cmds.push("quit".to_string());
        let mut args: Vec<String> = [
            "--mute",
            "play",
            "--headless",
            "--stage",
            "arena",
            "--class",
            self.weapon.class(),
            "--body",
            self.body.model(),
            "--head",
            self.body.head(),
            "--saber-rules",
            self.rules.name(),
        ]
        .map(String::from)
        .into();
        args.extend([
            "--moves".to_string(),
            moves.join("; "),
            "--audit-log".to_string(),
            dir.join(FRAMES).display().to_string(),
            "--cmds".to_string(),
            cmds.join(";"),
        ]);
        args
    }

    /// The environment of the case's run: the Jedi Academy install `ja`
    /// and the settings file in `dir`.
    fn env(&self, ja: &Path, dir: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("KJK_JA".to_string(), ja.display().to_string()),
            (
                "KJK_SETTINGS".to_string(),
                dir.join(SETTINGS).display().to_string(),
            ),
        ])
    }

    /// The case's states measured on `drawn`, its run's frames. Only the
    /// frames on the stepped grid are measured, one for each step
    /// (`super::stepped`). A state with no frame has no worst values. A
    /// state but `HOLSTER` whose frames have no lit blade has none either,
    /// as every blade check would pass it unmeasured. With the stances on,
    /// a state the stance swap covers (`swapped_by_stances`) with a lit
    /// blade, in which the drawn body never plays another sequence than the
    /// sim's, is a fallback: the skeleton lacks that state's sequence, so
    /// JA's played.
    pub fn measured(&self, drawn: Vec<super::Frame>) -> Vec<StateRecord> {
        let frames = super::stepped(drawn);
        timeline(SCRIPT)
            .1
            .into_iter()
            .map(|span| {
                let first = frames.partition_point(|frame| frame.ms < span.from as f64);
                let last = frames.partition_point(|frame| frame.ms < span.to as f64);
                let in_state = &frames[first..last];
                let lit_frames = in_state
                    .iter()
                    .filter(|frame| frame.blades.iter().any(|blade| !blade.growing))
                    .count();
                let no_lit_blade = !in_state.is_empty() && lit_frames == 0 && span.label != HOLSTER;
                let worst = if in_state.is_empty() || no_lit_blade {
                    Vec::new()
                } else {
                    measure(in_state, self.weapon == Weapon::Staff, span.still)
                };
                StateRecord {
                    state: span.label.to_string(),
                    frames: in_state.len(),
                    lit_frames,
                    no_lit_blade,
                    fallback: self.stances_on()
                        && swapped_by_stances(span.label)
                        && lit_frames > 0
                        && !in_state.iter().any(|frame| frame.swapped),
                    worst: worst.into_iter().map(Into::into).collect(),
                }
            })
            .collect()
    }
}

/// The file each run writes its frames into, in its folder.
const FRAMES: &str = "frames.jsonl";
/// The settings file each run reads, in its folder.
const SETTINGS: &str = "settings.json";
/// The file each run's error output goes to, in its folder.
const STDERR: &str = "stderr.log";

#[derive(clap::Args)]
pub struct AuditArgs {
    /// The folder to write `cases.json`, `report.md` and each run's folder
    /// into, else `context/artifacts/anim-audit/<YYYY-MM-DD-HHMM>` (UTC)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Only the cases whose tags hold every word of this comma list: a
    /// stance (`obi`, `ja` for the stances off), a weapon (`single`,
    /// `staff`, `dual`), a body (`male`, `female`), `ja-rules` or
    /// `mda-rules`, e.g. `obi,single,male`
    #[arg(long)]
    only: Option<String>,
    /// An earlier run's `cases.json`: say each row that got worse and each
    /// JA-rules value that changed at all, and fail if there is any
    #[arg(long)]
    against: Option<PathBuf>,
    /// The Jedi Academy install with Movie Duels that the runs read, else
    /// `kod/md-qa/ja` in the home folder
    #[arg(long)]
    ja: Option<PathBuf>,
    /// Film the worst failing rows into `clips/` of the output folder, a
    /// slowed gif each, and link them from `report.md`
    #[arg(long)]
    clips: bool,
    /// An earlier run's folder: measure again, by today's checks, the
    /// frames each case of its `cases.json` wrote into `runs/<case>/`,
    /// instead of playing the cases. Needs `--out`, another folder
    #[arg(long, requires = "out")]
    from_frames: Option<PathBuf>,
}

/// Runs the cases `args` choose and writes their report.
pub fn run(args: &AuditArgs) -> ExitCode {
    match audit(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("anim audit: {error}");
            ExitCode::FAILURE
        }
    }
}

fn audit(args: &AuditArgs) -> Result<ExitCode, String> {
    let old = match &args.against {
        Some(path) => Some(report::read(path)?),
        None => None,
    };
    let exe = std::env::current_exe().map_err(|error| format!("kjk's own path: {error}"))?;
    let (out, mut records) = match &args.from_frames {
        Some(dir) => remeasured(dir, args)?,
        None => played_all(&exe, args)?,
    };
    report::floor(&mut records);
    report::write(&out, &records)?;
    if args.clips {
        super::clips::film(&exe, &out, &records)?;
        // Written again so that its rows link the clips.
        report::write(&out, &records)?;
    }
    eprintln!("anim audit: {}", out.join(report::REPORT).display());
    let Some(old) = old else {
        return Ok(ExitCode::SUCCESS);
    };
    let worse = report::worse(&old, &records);
    for line in &worse {
        println!("{line}");
    }
    if worse.is_empty() {
        println!("anim audit: nothing got worse against the earlier run");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("anim audit: {} rows got worse or changed", worse.len());
        Ok(ExitCode::FAILURE)
    }
}

/// The items of `items` whose case (`case`) `--only` chooses. Choosing
/// none fails.
fn chosen<T>(
    items: Vec<T>,
    case: impl Fn(&T) -> &Case,
    only: Option<&str>,
) -> Result<Vec<T>, String> {
    let chosen: Vec<T> = items
        .into_iter()
        .filter(|item| only.is_none_or(|only| case(item).chosen(only)))
        .collect();
    if chosen.is_empty() {
        return Err(format!(
            "--only {}: no case has all those tags",
            only.unwrap_or("")
        ));
    }
    Ok(chosen)
}

/// The output folder `out`, made and absolute, so the arguments kept in
/// `cases.json` rerun a case from any folder.
fn out_folder(out: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out).map_err(|error| io(out, error))?;
    std::path::absolute(out).map_err(|error| io(out, error))
}

/// The output folder and the records of the cases `args` choose, each
/// played and measured.
fn played_all(exe: &Path, args: &AuditArgs) -> Result<(PathBuf, Vec<Record>), String> {
    let ja = match &args.ja {
        Some(ja) => ja.clone(),
        None => std::env::home_dir()
            .ok_or("no home folder: pass --ja")?
            .join("kod/md-qa/ja"),
    };
    let discovery = vfs::discover(|name| {
        if name == vfs::Game::JediAcademy.env_var() {
            Some(ja.display().to_string())
        } else {
            std::env::var(name).ok()
        }
    });
    let stances = kjk_game::duel_stances::offered_folders(&discovery)?;
    let chosen = chosen(cases(&stances), |case| case, args.only.as_deref())?;
    let out = out_folder(&args.out.clone().unwrap_or_else(default_out))?;
    eprintln!(
        "anim audit: {} cases into {}. The skeletons offered are {}.",
        chosen.len(),
        out.display(),
        stances.join(", ")
    );
    let records = run_all(exe, &ja, &out, &chosen);
    Ok((out, records))
}

/// The output folder and the records of the earlier run in `dir` (its
/// `cases.json`) that `args` choose. Each is measured again on the frames
/// its run wrote under `dir`, and nothing is played. A case whose run
/// failed keeps its failure. The output folder must be another one than
/// `dir`, which is only read.
fn remeasured(dir: &Path, args: &AuditArgs) -> Result<(PathBuf, Vec<Record>), String> {
    let out = args.out.as_deref().ok_or("--from-frames needs --out")?;
    let out = out_folder(out)?;
    let same = std::fs::canonicalize(dir).map_err(|error| io(dir, error))?
        == std::fs::canonicalize(&out).map_err(|error| io(&out, error))?;
    if same {
        return Err("--out must be another folder than --from-frames".to_string());
    }
    let earlier = report::read(&dir.join(report::CASES))?;
    let earlier = chosen(earlier, |record| &record.case, args.only.as_deref())?;
    eprintln!(
        "anim audit: {} cases of {} measured again into {}",
        earlier.len(),
        dir.display(),
        out.display()
    );
    let measured = at_once(&earlier, |record| {
        let dir = run_folder(dir, &record.case);
        record
            .failed
            .is_none()
            .then(|| measured_log(&record.case, &dir))
    });
    let records = earlier
        .into_iter()
        .zip(measured)
        .map(|(mut record, measured)| {
            match measured {
                Some(Ok(states)) => record.states = states,
                Some(Err(error)) => {
                    record.states = Vec::new();
                    record.failed = Some(error);
                }
                None => {}
            }
            record
        })
        .collect();
    Ok((out, records))
}

/// `context/artifacts/anim-audit/<YYYY-MM-DD-HHMM>`, the time in UTC.
fn default_out() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, seconds) = (now / 86_400, now % 86_400);
    let (year, month, day) = saves::civil_from_days(days as i64);
    let stamp = format!(
        "{year:04}-{month:02}-{day:02}-{:02}{:02}",
        seconds / 3600,
        seconds / 60 % 60
    );
    PathBuf::from("context/artifacts/anim-audit").join(stamp)
}

/// Every case of `cases` run and measured, `AT_ONCE` at a time, each in its
/// own folder under `out/runs`. The records keep the cases' order.
fn run_all(exe: &Path, ja: &Path, out: &Path, cases: &[Case]) -> Vec<Record> {
    let done = AtomicUsize::new(0);
    at_once(cases, |case| {
        let started = Instant::now();
        let record = run_case(exe, ja, out, case);
        let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!(
            "[{finished}/{}] {}: {} ({:.0} s)",
            cases.len(),
            record.name,
            record.failed.as_deref().map_or("measured", |_| "FAILED"),
            started.elapsed().as_secs_f32()
        );
        record
    })
}

/// `work` done on each of `items`, `AT_ONCE` at a time. The results keep
/// the items' order.
pub fn at_once<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..AT_ONCE.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = work(item);
                    // A lock poisoned by another worker's panic still holds
                    // the results the other workers made.
                    let mut results = results
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    results[index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .flatten()
        .collect()
}

/// `case` run in its folder and measured. A run that cannot start, exits
/// with an error, runs past `RUN_LIMIT` or writes no frame is a failed
/// case, with its error output's last lines.
fn run_case(exe: &Path, ja: &Path, out: &Path, case: &Case) -> Record {
    let name = case.name();
    let dir = run_folder(out, case);
    let args = case.args(&dir);
    let env = case.env(ja, &dir);
    let mut record = Record {
        case: case.clone(),
        name,
        args,
        env,
        failed: None,
        states: Vec::new(),
    };
    match played(exe, case, &dir, &record) {
        Ok(states) => record.states = states,
        Err(error) => record.failed = Some(error),
    }
    record
}

/// The folder of the run of `case` in the output folder `out`.
fn run_folder(out: &Path, case: &Case) -> PathBuf {
    out.join("runs").join(case.name().replace(' ', "-"))
}

/// The states of `case` measured on the frames its run writes in `dir`,
/// with `record`'s arguments and environment.
fn played(
    exe: &Path,
    case: &Case,
    dir: &Path,
    record: &Record,
) -> Result<Vec<StateRecord>, String> {
    std::fs::create_dir_all(dir).map_err(|error| io(dir, error))?;
    let settings = dir.join(SETTINGS);
    let text = case.settings().map_err(|error| io(&settings, error))?;
    std::fs::write(&settings, text).map_err(|error| io(&settings, error))?;
    let frames = dir.join(FRAMES);
    // A frames file left by an earlier run into the same folder must not
    // pass for this run's.
    let _ = std::fs::remove_file(&frames);
    run_kjk(exe, &record.args, &record.env, &dir.join(STDERR))?;
    measured_log(case, dir)
}

/// The states of `case` measured on the frames its run wrote in `dir`. A
/// log that is missing or has no frame fails, with the run's error
/// output's last lines.
fn measured_log(case: &Case, dir: &Path) -> Result<Vec<StateRecord>, String> {
    let frames = dir.join(FRAMES);
    let stderr_path = dir.join(STDERR);
    let text = std::fs::read_to_string(&frames)
        .map_err(|error| format!("no audit log: {}{}", io(&frames, error), tail(&stderr_path)))?;
    let parsed = super::parse(&text).map_err(|error| io(&frames, error))?;
    if parsed.is_empty() {
        return Err(format!("the audit log has no frame{}", tail(&stderr_path)));
    }
    Ok(case.measured(parsed))
}

/// A `kjk` run of `args` with `env`, its error output written to
/// `stderr_path`. A run that cannot start, exits with an error or runs past
/// `RUN_LIMIT` fails, with its error output's last lines.
pub fn run_kjk(
    exe: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    stderr_path: &Path,
) -> Result<(), String> {
    let stderr = std::fs::File::create(stderr_path).map_err(|error| io(stderr_path, error))?;
    let mut child = Command::new(exe)
        .args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| io(exe, error))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > RUN_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "stopped after {} s{}",
                    RUN_LIMIT.as_secs(),
                    tail(stderr_path)
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(error) => return Err(format!("waiting for the run: {error}")),
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err(format!("the run exited with {status}{}", tail(stderr_path)))
    }
}

/// The last lines of the error output in `path`, after a colon and a line
/// break, or nothing when it is empty or cannot be read.
fn tail(path: &Path) -> String {
    const LINES: usize = 20;
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        ":\n{}",
        lines[lines.len().saturating_sub(LINES)..].join("\n")
    )
}
