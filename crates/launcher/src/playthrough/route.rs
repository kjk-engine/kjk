//! A route: its goals parsed from text, its checkpoints and where a run
//! resumes.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Goal {
    Talk,
    Pick(Vec<u8>),
    /// Words a reply says (lowercase).
    Say(String),
    To(Target),
    Use(Target),
    Exit(Target),
    Fight(f32),
    /// Free slots filled, or one item (a template) worn.
    Equip(Option<String>),
    /// The lock, and a spike's template.
    Unlock(Target, Option<String>),
    /// A lock struck until it breaks.
    Bash(Target),
    /// A mine set on a door until it stands open (TSL).
    Mine(Target),
    /// The mine the party sees nearest a place disarmed (Demolitions) from
    /// where the player stands, tried again until it is gone.
    Disarm([f32; 2]),
    /// A power (`spells.2da` row) cast on the player from its power list,
    /// again until its effect lasts on it.
    Cast(u16),
    /// An item's template the party carries, put into the container.
    Put(String, Target),
    /// A script global set: a number, or a boolean.
    Global(String, Result<i32, bool>),
    /// The party's credits set.
    Gold(i32),
    /// The player's rank in a skill set (a build the route stands in for).
    Skill(usize, u8),
    /// The party installed in a module, arriving at a tag (none: where the
    /// module's own entry puts it).
    Goto(String, Option<String>),
    Leader(u8),
    /// Solo mode on or off.
    Solo(bool),
    /// The game won and over (`EndGame`'s end screen shown).
    Credits,
    /// What the party carries sold, dearest first, to the store open to
    /// the player until the party holds this much gold.
    Sell(i32),
    /// The companions (`NPC_*`) the next party selection takes.
    Party([u8; 2]),
    /// A planet (`planetary.2da` row) travelled to from the galaxy map.
    Travel(u8),
    LevelUp(u8),
    Wait(u64),
    /// Stand until a script global has the value, read as an `if` reads
    /// its own.
    Until(Condition),
    /// Stand until no object of the tag (the area's `N`th) is there: a
    /// script's `GetIsObjectValid(GetObjectByTag(..))` false, the dead taken
    /// away (`sim::remains`; their remains have no tag).
    Gone(String, Option<usize>),
    /// Stand until the companions have caught up with the player: each up
    /// and as near as a follower keeps to its leader (`ai::FOLLOW_NEAR`).
    Gather,
    /// The goals of the block after it (to its `end`) played only when a
    /// script global has the value then, the rest skipped: what a player
    /// does on seeing how things went (a race lost, raced again).
    If(Condition, usize),
    /// The end of a `while` block: back to its `while` (the goal there),
    /// at most `MAX_ROUNDS` times.
    Again(usize),
    Module(String),
    Save(String),
    /// A swoop race run to its end (the module its script leaves to).
    Race,
    /// The turret's fighters fought until the module changes.
    Turrets,
    /// A pazaak match a conversation started, played out.
    Pazaak,
}

/// What a walking goal names: an object by tag (or template), with the
/// number `TAG@N` gives it, or a place (metres).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Target {
    Tag(String, Option<usize>),
    Place([f32; 2]),
    /// `item:TEMPLATE`: what in the module holds an item of that template,
    /// remains or a creature (the dead of a route before leave remains with
    /// no tag to name them by).
    Holding(String),
}

pub(super) fn target(word: Option<&str>, rest: Option<&str>) -> Result<Target, String> {
    let word = word.ok_or("a tag or a place")?;
    if let Some(template) = word.strip_prefix("item:") {
        return Ok(Target::Holding(template.to_ascii_lowercase()));
    }
    // A place as X,Y too.
    if let Some((x, y)) = word.split_once(',')
        && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
    {
        return Ok(Target::Place([x, y]));
    }
    if let Ok(x) = word.parse::<f32>() {
        let y = rest
            .and_then(|y| y.parse::<f32>().ok())
            .ok_or(format!("{word}: a place is two numbers"))?;
        return Ok(Target::Place([x, y]));
    }
    let (tag, nth) = match word.split_once('@') {
        Some((tag, nth)) => (
            tag,
            Some(
                nth.parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or(format!("{word}: TAG@N, N from 1"))?,
            ),
        ),
        None => (word, None),
    };
    Ok(Target::Tag(tag.to_string(), nth))
}

/// The checkpoints `route` saves, by name, each with whether it may be
/// saved again (twice in the route, or in a `while`): a chain starts the
/// routes after one once its file is there, or else when the route ends.
pub(crate) fn saves(route: &str) -> Result<BTreeMap<String, bool>, String> {
    let goals = parse(route)?;
    let looped = |at: usize| {
        goals.iter().enumerate().any(|(end, (_, goal))| {
            matches!(goal, Goal::Again(start) if *start < at && at < end)
        })
    };
    let mut saves = BTreeMap::new();
    for (at, (_, goal)) in goals.iter().enumerate() {
        if let Goal::Save(name) = goal {
            saves
                .entry(name.clone())
                .and_modify(|again| *again = true)
                .or_insert_with(|| looped(at));
        }
    }
    Ok(saves)
}

/// The goals of a route (see the module's notes).
pub(super) fn parse(route: &str) -> Result<Vec<(String, Goal)>, String> {
    let mut goals = Vec::new();
    // The `if` and `while` goals whose blocks are open (`while`: true).
    let mut open: Vec<(usize, bool)> = Vec::new();
    for line in route.lines() {
        let line = line.split('#').next().unwrap_or("");
        for part in line.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let mut words = part.split_whitespace();
            let verb = words.next().unwrap_or("");
            let (a, b) = (words.next(), words.next());
            let goal = match verb {
                "talk" => Goal::Talk,
                "pick" => Goal::Pick(
                    a.unwrap_or("")
                        .split(',')
                        .map(|n| n.parse::<u8>().ok().filter(|n| *n >= 1))
                        .collect::<Option<Vec<u8>>>()
                        .ok_or(format!("{part}: pick 1,2,…"))?,
                ),
                "say" => Goal::Say(
                    part.split_once(char::is_whitespace)
                        .map(|(_, words)| words.trim().to_lowercase())
                        .filter(|words| !words.is_empty())
                        .ok_or(format!("{part}: say WORDS"))?,
                ),
                "travel" => Goal::Travel(
                    a.and_then(|n| n.parse::<u8>().ok())
                        .ok_or(format!("{part}: travel PLANET"))?,
                ),
                "party" => {
                    let picks = a
                        .unwrap_or("")
                        .split(',')
                        .map(|n| n.parse::<u8>().ok())
                        .collect::<Option<Vec<u8>>>()
                        .filter(|picks| (1..=2).contains(&picks.len()))
                        .ok_or(format!("{part}: party NPC[,NPC]"))?;
                    Goal::Party([picks[0], picks.get(1).copied().unwrap_or(u8::MAX)])
                }
                "to" => Goal::To(target(a, b)?),
                "at" => Goal::To(target(a, b)?),
                "use" => Goal::Use(target(a, b)?),
                "exit" => Goal::Exit(target(a, b)?),
                "fight" => Goal::Fight(a.map_or(Ok(15.0), |r| {
                    r.parse::<f32>()
                        .map_err(|_| format!("{part}: fight METRES"))
                })?),
                "wait" => Goal::Wait(
                    a.and_then(|w| w.strip_suffix('s'))
                        .and_then(|s| s.parse::<f32>().ok())
                        .map(|s| (s * 1000.0) as u64)
                        .ok_or(format!("{part}: wait 5s"))?,
                ),
                "until" | "if" | "while" => {
                    let condition = match a {
                        Some("in") => Condition::In(
                            b.ok_or(format!("{part}: {verb} in MODULE"))?
                                .to_ascii_lowercase(),
                        ),
                        Some("npc") => {
                            let (npc, truth) = b
                                .and_then(|b| b.split_once('='))
                                .and_then(|(npc, truth)| Some((npc.parse::<u8>().ok()?, truth)))
                                .filter(|(_, truth)| matches!(*truth, "0" | "1"))
                                .ok_or(format!("{part}: {verb} npc NUMBER=0|1"))?;
                            Condition::Npc(npc, truth == "1")
                        }
                        _ => {
                            let (name, value) = a
                                .and_then(|a| a.split_once('='))
                                .and_then(|(name, value)| Some((name, value.parse::<i32>().ok()?)))
                                .ok_or(format!("{part}: {verb} NAME=NUMBER"))?;
                            Condition::Global(name.to_string(), value)
                        }
                    };
                    if verb == "until" {
                        Goal::Until(condition)
                    } else {
                        open.push((goals.len(), verb == "while"));
                        Goal::If(condition, 0)
                    }
                }
                "gone" => match target(a, None)? {
                    Target::Tag(tag, nth) => Goal::Gone(tag, nth),
                    Target::Place(_) | Target::Holding(_) => return Err(format!("{part}: gone TAG")),
                },
                "gather" => Goal::Gather,
                "end" => {
                    let (at, again) = open.pop().ok_or(format!("{part}: end with no if"))?;
                    if again {
                        goals.push((part.to_string(), Goal::Again(at)));
                    }
                    let block = goals.len() - at - 1;
                    if let Some((_, Goal::If(_, length))) = goals.get_mut(at) {
                        *length = block;
                    }
                    continue;
                }
                "levelup" => Goal::LevelUp(
                    a.map_or(Ok(0), |class| class.parse::<u8>())
                        .map_err(|_| format!("{part}: levelup CLASS"))?,
                ),
                "leader" => Goal::Leader(match a {
                    Some("player") => u8::MAX,
                    npc => npc
                        .and_then(|n| n.parse::<u8>().ok())
                        .ok_or(format!("{part}: leader NPC or leader player"))?,
                }),
                "credits" => Goal::Credits,
                "sell" => Goal::Sell(
                    a.and_then(|gold| gold.parse::<i32>().ok())
                        .ok_or(format!("{part}: sell GOLD"))?,
                ),
                "solo" => Goal::Solo(match a {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err(format!("{part}: solo on or solo off")),
                }),
                "equip" => Goal::Equip(a.map(str::to_ascii_lowercase)),
                "unlock" => Goal::Unlock(target(a, None)?, b.map(str::to_string)),
                "bash" => Goal::Bash(target(a, b)?),
                "mine" => Goal::Mine(target(a, b)?),
                "disarm" => match target(a, b)? {
                    Target::Place(at) => Goal::Disarm(at),
                    Target::Tag(..) | Target::Holding(_) => {
                        return Err("disarm: a place (X,Y), the mine nearest it".into());
                    }
                },
                "cast" => Goal::Cast(
                    a.and_then(|row| row.parse::<u16>().ok())
                        .ok_or(format!("{part}: cast ROW (spells.2da)"))?,
                ),
                "put" => Goal::Put(
                    a.ok_or(format!("{part}: put TEMPLATE TAG"))?
                        .to_ascii_lowercase(),
                    target(b, None)?,
                ),
                "module" => Goal::Module(a.ok_or(format!("{part}: module NAME"))?.to_string()),
                "race" => Goal::Race,
                "turrets" => Goal::Turrets,
                "pazaak" => Goal::Pazaak,
                "save" => Goal::Save(a.ok_or(format!("{part}: save NAME"))?.to_string()),
                "gold" => Goal::Gold(
                    a.and_then(|gold| gold.parse::<i32>().ok())
                        .ok_or(format!("{part}: gold N"))?,
                ),
                "global" => {
                    let (name, value) = a
                        .and_then(|a| a.split_once('='))
                        .ok_or(format!("{part}: global NAME=VALUE"))?;
                    let value = match value {
                        "true" => Err(true),
                        "false" => Err(false),
                        number => Ok(number
                            .parse::<i32>()
                            .map_err(|_| format!("{part}: global NAME=NUMBER or true/false"))?),
                    };
                    Goal::Global(name.to_string(), value)
                }
                "skill" => {
                    let (row, rank) = crate::by_hand::parse_skill(
                        a.ok_or(format!("{part}: skill NAME=RANK"))?,
                    )?;
                    Goal::Skill(row, rank)
                }
                "goto" => Goal::Goto(
                    a.ok_or(format!("{part}: goto MODULE [TAG]"))?
                        .to_ascii_lowercase(),
                    b.map(str::to_string),
                ),
                _ => return Err(format!("{part}: {verb}: not a goal")),
            };
            goals.push((part.to_string(), goal));
        }
    }
    if !open.is_empty() {
        return Err(format!("{} if with no end", open.len()));
    }
    Ok(goals)
}

/// Where the checkpoint `name` is kept: a path as given, else under the
/// game's checkpoints.
pub(crate) fn checkpoint(game: &str, name: &str) -> PathBuf {
    if name.ends_with(".kjkv") {
        PathBuf::from(name)
    } else {
        crate::runtime::artifacts()
            .join("checkpoints")
            .join(game)
            .join(format!("{name}.kjkv"))
    }
}

/// The world a checkpoint saved, in its module (planned again here). One
/// made with other level data is refused, unless `stale`: then its module
/// takes this level's data as it stands and the modules it left are
/// forgotten (met anew), so a route goes on at the chain's top while the
/// chain is remade below it.
pub(super) fn resume(
    installer: &Installer,
    rate: &TickRate,
    path: &Path,
    stale: bool,
) -> Result<sim::SimWorld, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut save = sim::Save::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if save.hz != rate.hz() {
        return Err(format!("{}: saved at {} Hz", path.display(), save.hz));
    }
    let module = save
        .snapshot
        .place
        .as_ref()
        .map(|place| place.module.clone())
        .ok_or(format!("{}: saved off a module", path.display()))?;
    let request = installer.request_from(&save.snapshot, &module, None);
    let level = installer.prepare(&request, false)?.plan.level;
    if stale {
        // The modules it left may have other data now too: met anew.
        save.snapshot.stash = sim::Stash::default();
        if save.level.check(&level).is_err() {
            eprintln!(
                "{}: made with other level data; taken as it stands (--stale)",
                path.display()
            );
            save.snapshot.place = level.place();
            save.level = level.id();
        }
    }
    save.level.check(&level).map_err(|e| e.to_string())?;
    let mut world =
        sim::SimWorld::from_snapshot(&save.snapshot, level).map_err(|e| e.to_string())?;
    world.set_script_state(save.script);
    Ok(world)
}
