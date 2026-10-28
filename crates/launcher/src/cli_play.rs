//! `kjk play`: its flags and its run, and the sheet `--class` names (also
//! `kjk host`'s).

use std::process::ExitCode;

use crate::{by_hand, discover, installs, launch, persona, play, play_view, runtime};

#[derive(clap::Args)]
pub(crate) struct PlayArgs {
    /// The stage: `course`, `duel` (two players facing off), or a KOTOR 1
    /// module such as `end_m01aa`
    #[arg(long, default_value = "course")]
    stage: String,
    /// On a module stage: arrive at this door or waypoint (by tag)
    /// instead of the module's entry
    #[arg(long)]
    arrive: Option<String>,
    /// Each seat's KOTOR body model in turn (a comma list), `ja` for
    /// JA's own player model, or `roster` for the party's bodies
    /// (`play::ROSTER`: Carth, Bastila, Canderous, …); by default each
    /// seat's player as made (`--made`) or given (a player portrait's
    /// body), dressed in the body armour it wears
    #[arg(long, default_value = "")]
    body: String,
    /// Play as a character made by quick creation:
    /// CLASS:PORTRAIT[:NAME], the class `soldier`, `scout` or
    /// `scoundrel` (K1's), `guardian`, `consular` or `sentinel` (TSL's
    /// Jedi), a player portrait (`portraits.2da`, e.g. `po_pfha2`):
    /// the class's recommended scores and skills, its first level as
    /// the game makes it (the sheet `--class` names is not used)
    #[arg(long, value_parser = parse_made)]
    made: Option<(u8, String, String)>,
    /// Each body's KOTOR head model (a comma list); a party body's own
    /// by default
    #[arg(long, default_value = "")]
    head: String,
    /// Scripted input instead of the keyboard (see `moves.rs`), or a
    /// named script: `course`
    #[arg(long)]
    moves: Option<String>,
    /// `third` (JA's camera), `first` (the player's own eyes; F1 turns
    /// between them), `side` to film the gait from 3 m, `arc` the same
    /// at a fixed height (a jump's arc)
    #[arg(long)]
    camera: Option<String>,
    /// Reproduce the jolting reference quirks (QUIRKS.md): playback's
    /// (Q-0003, Q-0004, Q-0010, Q-0017, Q-0028), the look's (Q-0012 to
    /// Q-0014) and the camera's (Q-0016)
    #[arg(long)]
    quirks: bool,
    /// Write the body's pelvis, head, hands and feet every frame (CSV)
    #[arg(long)]
    trace: Option<std::path::PathBuf>,
    /// Turn the first module's creatures with this tag hostile before
    /// it is installed (repeatable)
    #[arg(long)]
    hostile: Vec<String>,
    /// Turn the first module's creatures with this tag to the player's
    /// side before it is installed (repeatable)
    #[arg(long)]
    ally: Vec<String>,
    /// Every player's sheet until the party table: `soldier` (a level-1
    /// soldier), `droid` (the soldier as a droid: what droids alone
    /// use, their utility items and shields), `jedi` (a level-6
    /// Guardian who knows every power the sim maps) or `master` (the
    /// same Jedi, who also throws its saber: Advanced Throw
    /// Lightsaber); `:N` sets its Jedi Defense tier (0
    /// to 3), e.g. `jedi:3`; `@ROW` puts the blaster of that
    /// `baseitems.2da` row in its first hand, e.g. `soldier@77`
    #[arg(long, default_value = "soldier")]
    class: String,
    /// Commands to run, e.g. 'anim 2.5; screenshot run; quit'
    #[arg(long)]
    cmds: Option<String>,
    #[arg(long)]
    headless: bool,
    /// Draw the sim's blades (green) beside the drawn ones, and the
    /// gaps between their tips (yellow), measured into the dumps and
    /// `blade-gap.csv`
    #[arg(long)]
    sim_blades: bool,
    /// Play this recording back on its own stage and seats (a probe's
    /// bout, `kjk combat duel`'s `*.kjkr`; a session's `inputs.kjkr`
    /// made off a module) instead of `--stage` and `--moves`
    #[arg(long)]
    replay: Option<std::path::PathBuf>,
    /// Watch `--replay` from this seat's player (0 by default): the
    /// other side of a probe's bout
    #[arg(long, default_value_t = 0, requires = "replay")]
    seat: u8,
    /// Turn the scripted player's view, every tick, to the chest of the
    /// nearest creature with this tag, the living first, then the body:
    /// fights that hit, and the camera on the fall, for filming them.
    /// `race` or `turrets`: the playthrough's bot plays the minigame
    #[arg(long)]
    aim: Option<String>,
    /// A script global set before the first step, NAME=NUMBER (a story's
    /// state by hand, until saves carry it); repeatable
    #[arg(long = "global", value_parser = by_hand::parse_global)]
    globals: Vec<(String, i32)>,
    /// A script of the first module run for the player at the end of
    /// the first step, by resref (a story's moment by hand: e.g.
    /// `k_ptar_carth01x` wakes Carth in `tar_m02aa`); repeatable
    #[arg(long = "run")]
    scripts: Vec<String>,
    /// The first module's creatures with this tag follow the player from
    /// the start, as party members (a party by hand, until saves carry
    /// one), into the next module too; repeatable
    #[arg(long = "follow")]
    followers: Vec<String>,
    /// The party's gold at the start (credits by hand, until saves
    /// load)
    #[arg(long)]
    gold: Option<i32>,
    /// The player's alignment at the start, 0 (dark) to 100 (light),
    /// as scripts' `SetGoodEvilValue` (the character sheet's backdrop)
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
    alignment: Option<u8>,
    /// The party's XP at the start (by hand, until saves load)
    #[arg(long)]
    xp: Option<i32>,
    #[command(flatten)]
    by_hand: Box<ByHand>,
}

/// A player's ranks and the party's items by hand (`kjk play`, `kjk
/// dialogue run`).
#[derive(clap::Args)]
pub(crate) struct ByHand {
    /// Every player's rank in a skill, NAME=RANK (computer, demolitions,
    /// stealth, awareness, persuade, repair, security, treat); repeatable
    #[arg(long = "skill", value_parser = by_hand::parse_skill)]
    pub(crate) skills: Vec<(usize, u8)>,
    /// Items the party carries from the start, TEMPLATE or TEMPLATE=COUNT
    /// (e.g. g_i_progspike01=5); repeatable
    #[arg(long = "item", value_parser = by_hand::parse_item)]
    pub(crate) items: Vec<(String, u16)>,
    /// The player talks to the object with this tag at the start, as
    /// using it would: stood 1.5 m from it first when farther than talking
    /// range, then its dialogue hook (a terminal's conversation)
    #[arg(long)]
    pub(crate) talk: Option<String>,
    /// A companion made from this creature template joined to the party
    /// at the start, as the party selection screen leaves it (e.g.
    /// `p_cand`, Canderous); repeatable, two at most
    #[arg(long = "party")]
    pub(crate) party: Vec<String>,
}

/// The classes `--made` names, by `classes.2da` row: K1's creation's,
/// then TSL's Jedi.
const MADE_CLASSES: [&str; 6] = [
    "soldier",
    "scout",
    "scoundrel",
    "guardian",
    "consular",
    "sentinel",
];

/// `--made`'s CLASS:PORTRAIT[:NAME] (`classes.2da` rows 0 to 5).
pub(crate) fn parse_made(text: &str) -> Result<(u8, String, String), String> {
    let mut parts = text.splitn(3, ':');
    let named = parts.next().unwrap_or("");
    let class = MADE_CLASSES
        .iter()
        .position(|class| *class == named)
        .ok_or(format!("{named}: one of {}", MADE_CLASSES.join(", ")))? as u8;
    let portrait = parts
        .next()
        .filter(|portrait| !portrait.is_empty())
        .ok_or("CLASS:PORTRAIT[:NAME]")?;
    let name = parts.next().unwrap_or("");
    Ok((class, portrait.to_ascii_lowercase(), name.to_string()))
}

/// `kjk play`: the stage (or a recording) loaded while the window opens.
pub(crate) fn run(args: PlayArgs, size: (u32, u32)) -> ExitCode {
    let PlayArgs {
        stage,
        arrive,
        body,
        made,
        head,
        moves,
        camera,
        quirks,
        trace,
        hostile,
        ally,
        class,
        cmds,
        headless,
        sim_blades,
        replay,
        seat,
        aim,
        globals,
        scripts,
        followers,
        gold,
        alignment,
        xp,
        by_hand,
    } = args;
    let ByHand {
        skills,
        items,
        talk,
        party,
    } = *by_hand;
    let replay = match replay.map(|path| read_recording(&path)).transpose() {
        Ok(replay) => replay,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let camera = match camera.as_deref() {
        None => None,
        Some("third") => Some(play_view::CameraMode::Third),
        Some("first") => Some(play_view::CameraMode::First),
        Some("side") => Some(play_view::CameraMode::Side(3.0)),
        Some("arc") => Some(play_view::CameraMode::Arc(3.0)),
        Some(other) => {
            eprintln!("--camera {other}: use third, first, side or arc");
            return ExitCode::from(2);
        }
    };
    let bodies = match play::bodies(&body, &head) {
        Ok(bodies) => bodies,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let options = play::Options {
        aim: aim.as_deref(),
        stage: &stage,
        arrive: arrive.as_deref(),
        bodies,
        moves: moves.as_deref(),
        camera,
        quirks,
        trace: trace.as_deref(),
        hostile: &hostile,
        allies: &ally,
        seats: 1,
        guests: sim::Guests::Avatars,
        // The arena's own Jedi unless another class is asked for.
        sheet: match (stage == "arena" && class == "soldier")
            .then(kjk_game::arena::sheet)
            .map_or_else(|| player_sheet(&class), Ok)
        {
            Ok(sheet) => sim::combat::Sheet {
                good_evil: alignment.unwrap_or(sheet.good_evil),
                ..by_hand::with_skills(sheet, &skills)
            },
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(2);
            }
        },
        sim_blades,
        replay,
        seat,
        globals: &globals,
        scripts: &scripts,
        followers: &followers,
        gold,
        xp,
        items: &items,
        talk: talk.as_deref(),
        party: &party,
        save: None,
        made: made
            .as_ref()
            .map(|(class, portrait, name)| persona::Made::Quick {
                class: *class,
                portrait,
                name,
            }),
    };
    runtime::open_screens(installs::game_of_module(&discover(), options.stage));
    launch(
        cmds.as_deref().unwrap_or(""),
        headless,
        size,
        Some(options.stage),
        || play::load(&discover(), &options),
    )
}

/// The recording in the file at `path`.
pub(crate) fn read_recording(path: &std::path::Path) -> Result<sim::Recording, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    sim::Recording::decode(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// The sheet `--class` names: `soldier`, `jedi` or `master`, after a colon
/// the Jedi Defense tier it has instead of its own (`jedi:3`), and after an
/// `@` the weapon (a `baseitems.2da` row) its first hand holds instead: a
/// blaster with the same numbers (`soldier@77`, a blaster rifle), a melee
/// weapon with a vibroblade's (1d10, slashing; `jedi@5`, a vibroblade).
pub(crate) fn player_sheet(class: &str) -> Result<sim::combat::Sheet, String> {
    let usage = || {
        format!(
            "--class {class}: soldier, droid, jedi or master, then :0 to :3 for its Jedi Defense, \
             then @ and a blaster's or melee weapon's baseitems.2da row"
        )
    };
    let (class, blaster) = match class.split_once('@') {
        Some((class, row)) => {
            let row = row.parse::<u16>().map_err(|_| usage())?;
            let delivery = sim::combat::Delivery::of_base_item(row)
                .filter(|delivery| delivery.ranged() || *delivery == sim::combat::Delivery::Melee)
                .ok_or_else(usage)?;
            (class, Some((row, delivery)))
        }
        None => (class, None),
    };
    let (name, defence) = match class.split_once(':') {
        Some((name, tier)) => (
            name,
            Some(
                tier.parse::<usize>()
                    .ok()
                    .filter(|t| *t <= 3)
                    .ok_or_else(usage)?,
            ),
        ),
        None => (class, None),
    };
    let mut sheet = match name {
        "soldier" => sim::combat::Sheet::dev_soldier(),
        "droid" => sim::combat::Sheet {
            race: sim::combat::sheet::races::DROID,
            ..sim::combat::Sheet::dev_soldier()
        },
        "jedi" => sim::combat::Sheet::dev_jedi(),
        "master" => {
            let mut sheet = sim::combat::Sheet::dev_jedi();
            // Advanced Throw Lightsaber (`spells.2da` row 4).
            sheet.powers.insert(4);
            sheet
        }
        _ => return Err(usage()),
    };
    if let Some((row, delivery)) = blaster {
        let weapon = sheet.weapons[0].as_mut().ok_or_else(usage)?;
        if delivery.ranged() {
            weapon.base_item = row;
            weapon.delivery = delivery;
            weapon.reach_m = sim::combat::Weapon::reach_of(row);
            weapon.build = sim::combat::Weapon::build_of(row);
        } else {
            use sim::combat::sheet::{damage_types, feats};
            *weapon = sim::combat::Weapon::plain(
                row,
                1,
                10,
                (2, 2),
                damage_types::SLASHING,
                [
                    feats::PROFICIENCY_MELEE,
                    feats::WEAPON_FOCUS_MELEE,
                    feats::WEAPON_SPEC_MELEE,
                ],
            );
        }
    }
    Ok(match defence {
        Some(tier) => by_hand::with_defence(sheet, tier),
        None => sheet,
    })
}
