//! `kjk host`: its flags and its run.

use std::process::ExitCode;

use crate::cli_play::player_sheet;
use crate::{
    ByHandParty, HostSpec, by_hand, discover, host_session, launch, net_join, netem_conditions,
    save_view,
};

#[derive(clap::Args)]
pub(crate) struct HostArgs {
    /// The stage: `course`, `duel` (two players facing off), or a KOTOR 1
    /// module such as `end_m01aa`
    #[arg(long, default_value = "course")]
    stage: String,
    /// On a module stage: arrive at this door or waypoint (by tag)
    #[arg(long)]
    arrive: Option<String>,
    /// The seats the world has: this player and those who join; three
    /// (I30 P1: KOTOR's party) when guests play companions, two when
    /// each plays an avatar (every seat's stands from the start)
    #[arg(long, value_parser = parse_seats)]
    seats: Option<u8>,
    /// What those who join play: `companions`, the party's companions
    /// (a follow camera while none is free), or `avatars`, a player
    /// each; `companions` on a module, `avatars` on the course, the duel
    /// floor and the ring unless told
    #[arg(long, value_parser = parse_guests)]
    guests: Option<sim::Guests>,
    /// The first module's creatures with this tag follow the host from
    /// the start, as party members, as `kjk play --follow` (a companion
    /// for a guest to drive); repeatable
    #[arg(long = "follow")]
    followers: Vec<String>,
    /// The UDP port players join on
    #[arg(long, default_value_t = net_join::PORT)]
    port: u16,
    /// Turn this player's scripted view, every tick, to the nearest
    /// creature with this tag, as `kjk play --aim`
    #[arg(long)]
    aim: Option<String>,
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
    /// The address to listen on (`::`: IPv6 and IPv4)
    #[arg(long, default_value = "::")]
    bind: String,
    /// Network conditions on what this side sends, e.g.
    /// 'delay=50ms,loss=2%' (overrides KJK_NETEM)
    #[arg(long)]
    netem: Option<String>,
    /// Scripted input instead of the keyboard (see `moves.rs`)
    #[arg(long)]
    moves: Option<String>,
    /// Commands to run, e.g. 'wait 10s; quit'
    #[arg(long)]
    cmds: Option<String>,
    #[arg(long)]
    headless: bool,
    /// Draw the sim's blades (green) beside the drawn ones, and the
    /// gaps between their tips (yellow), measured into the dumps and
    /// `blade-gap.csv`
    #[arg(long)]
    sim_blades: bool,
    /// The party's gold at the start (credits by hand, until saves
    /// load)
    #[arg(long)]
    gold: Option<i32>,
    /// Items the party carries from the start, TEMPLATE or
    /// TEMPLATE=COUNT; repeatable
    #[arg(long = "item", value_parser = by_hand::parse_item)]
    items: Vec<(String, u16)>,
    /// The party's XP at the start (by hand, until saves load)
    #[arg(long)]
    xp: Option<i32>,
    /// A script global set before the first step, NAME=NUMBER, as `kjk
    /// play --global` (a race cold: `TAR_SWOOP_ACCEL=7`); repeatable
    #[arg(long = "global", value_parser = by_hand::parse_global)]
    globals: Vec<(String, i32)>,
    /// Host the save with this id (`quick`, `0001`, `r0001`, ... in the
    /// saves folder, `KJK_SAVES`) instead of the stage: its module and
    /// world, for `--seats` (its guests take its companions)
    #[arg(long)]
    load: Option<String>,
    /// Only players who give this password join (`kjk join
    /// --password`); it is never sent, only proved
    #[arg(long)]
    password: Option<String>,
    /// Steam friends join too, through Steam's relays (no port opened):
    /// from their friends list, or `kjk join steam:<id>`. The menu's host
    /// always offers it; scripted hosts only when asked
    #[arg(long)]
    steam: bool,
}

pub(crate) fn parse_seats(text: &str) -> Result<u8, String> {
    text.parse::<u8>()
        .ok()
        .filter(|seats| *seats <= sim::MAX_SEATS)
        .ok_or(format!("{text}: seats from 0 to {}", sim::MAX_SEATS))
}

fn parse_guests(text: &str) -> Result<sim::Guests, String> {
    match text {
        "avatars" => Ok(sim::Guests::Avatars),
        "companions" => Ok(sim::Guests::Companions),
        other => Err(format!("{other}: avatars or companions")),
    }
}

/// What guests play on `stage` unless told: the party's companions in a
/// module, a player each on the course, the duel floor and the ring.
fn guests_on(stage: &str) -> sim::Guests {
    match stage {
        "course" | "duel" | "ring" => sim::Guests::Avatars,
        _ => sim::Guests::Companions,
    }
}

/// The seats a host has unless told: KOTOR's party of three (I30 P1) when
/// guests take its companions, a seat without a guest having no body; two
/// when every seat stands as a player of its own from the start (the duel
/// floor, the course), so none stands idle.
fn seats_for(guests: sim::Guests) -> u8 {
    match guests {
        sim::Guests::Avatars => 2,
        sim::Guests::Companions => 3,
    }
}

/// `kjk host`: the stage for `seats` players, listening for them.
pub(crate) fn run(args: HostArgs, size: (u32, u32)) -> ExitCode {
    let conditions = match netem_conditions(args.netem.as_deref()) {
        Ok(conditions) => conditions,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let Ok(ip) = args.bind.parse::<std::net::IpAddr>() else {
        eprintln!("--bind {}: not an IP address", args.bind);
        return ExitCode::from(2);
    };
    let sheet = match player_sheet(&args.class) {
        Ok(sheet) => sheet,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let saved = args
        .load
        .as_deref()
        .map(|id| save_view::read(id).map(|(save, meta)| (save, meta.module)))
        .transpose();
    let saved = match saved {
        Ok(saved) => saved,
        Err(error) => {
            eprintln!("--load: {error}");
            return ExitCode::from(2);
        }
    };
    let stage = saved
        .as_ref()
        .map_or(args.stage.as_str(), |(_, module)| module);
    let guests = args.guests.unwrap_or_else(|| guests_on(stage));
    let seats = args.seats.unwrap_or_else(|| seats_for(guests));
    if !(1..=sim::MAX_SEATS).contains(&seats) {
        eprintln!("--seats: 1 to {}", sim::MAX_SEATS);
        return ExitCode::from(2);
    }
    let spec = HostSpec {
        stage,
        arrive: args.arrive.as_deref(),
        seats,
        guests,
        address: std::net::SocketAddr::new(ip, args.port),
        sheet,
        conditions,
        moves: args.moves.as_deref(),
        aim: args.aim.as_deref(),
        sim_blades: args.sim_blades,
        save: saved.as_ref().map(|(save, _)| save),
        party: ByHandParty {
            followers: &args.followers,
            gold: args.gold,
            items: &args.items,
            xp: args.xp,
            globals: &args.globals,
        },
        password: args.password.as_deref(),
        made: None,
        steam: args.steam,
    };
    launch(
        args.cmds.as_deref().unwrap_or(""),
        args.headless,
        size,
        Some(stage),
        || host_session(&discover(), spec),
    )
}
